//! Read-only upstream conversion service. The host authorizes this declared operation;
//! only this adapter knows Comfy routes or metadata. No inference is submitted here.
extern crate alloc;
use alloc::{format, string::ToString, vec, vec::Vec};
use serde_json::{json, Value};
use bgx_adapter_v2_sdk::{Host, Invocation};
use crate::{comfy, mapping::Failure};

pub const OPERATION: &str = "local.thirdparty.comfy-panorama-image/editor-3d-preview";
pub const MODEL: &str = "panorama-editor-media";
pub const CAPABILITY: &str = "local.thirdparty.comfy-panorama-image/editor-media";

pub fn contract(config: &Value) -> Value {
    let input = crate::capability::schema("editor-media-input", include_str!("../schemas/editor-media-input.json"));
    let output = crate::capability::schema("editor-media-output", include_str!("../schemas/editor-media-output.json"));
    json!({"schemaVersion":2,"localId":MODEL,"capabilityKey":CAPABILITY,"transportSlot":"generic","modelIds":[MODEL],
      "configurationSchema":{"id":"config","digest":config["digest"]},
      "operations":[{"id":OPERATION,"version":1,"label":"3D素材预览转换",
        "inputSchema":{"id":"editor-media-input","digest":input["digest"]},
        "outputSchema":{"id":"editor-media-output","digest":output["digest"]},"resumeSchema":null,
        "uiSchema":{"controls":[]},"mediaSlots":[{"role":"sources","inputPointer":"/sources","mediaKind":"file","minCount":1,"maxCount":1}],
        "execution":{"mode":"synchronous","streaming":false,"cancellation":"unsupported","resumable":false,"remoteIdempotency":false},
        "outputSlots":[{"role":"preview","kind":"file","minCount":1,"maxCount":1},{"role":"animation","kind":"file","minCount":0,"maxCount":1}]}],
      "schemas":[input,output,config]})
}

pub fn submit(host: &mut dyn Host, request: &Invocation, payload: &Value) -> Result<Value, Failure> {
    if payload["operationId"] != OPERATION || payload["operationVersion"] != 1 || payload["modelId"] != MODEL || payload["capabilityKey"] != CAPABILITY {
        return Err(Failure::invalid("Editor media operation identity differs"));
    }
    let input = &payload["input"];
    let name = input["fileName"].as_str().ok_or_else(|| Failure::invalid("fileName required"))?;
    let extension = name.rsplit_once('.').map(|(_,ext)|ext.to_ascii_lowercase()).unwrap_or_default();
    // Exact upstream file readers; no arbitrary local path, URL or script input.
    if name.contains(['/', '\\', ':']) || !["glb","gltf","obj","stl","fbx","usdz","ply","splat","spz","ksplat"].contains(&extension.as_str()) {
        return Err(Failure::invalid("Unsupported upstream 3D file format"));
    }
    let sources = input["sources"].as_array().filter(|v|v.len()==1).ok_or_else(||Failure::invalid("Exactly one source is required"))?;
    let media = payload["media"].as_array().and_then(|items|items.iter().find(|item|item["id"]==sources[0]["mediaId"] && item["kind"]=="file"))
        .ok_or_else(||Failure::invalid("Missing owned file lease"))?;
    let mut uploaded_media = media.clone();
    uploaded_media["fileName"] = json!(format!("{}.{extension}", request.r#request_id));
    let ctx = comfy::Ctx { config:&payload["configuration"],budget_ms:request.r#budget_ms };
    let uploaded = comfy::upload(host, &ctx, &uploaded_media)?;
    let (folder,name) = uploaded.rsplit_once('/').unwrap_or(("",uploaded.as_str()));
    let body=serde_json::to_vec(&json!({"filename":name,"subfolder":folder,"storage":"input"})).expect("source");
    let mut metadata=comfy::exchange(host,&ctx,"POST","/panorama_suite/file_3d",json!({"kind":"bytes","bytes":body}),
        vec![json!({"name":"Content-Type","value":"application/json"})],false)?;
    if !matches!(metadata["kind"].as_str(),Some("mesh"|"splat")) { return Err(Failure::invalid("Unexpected converted scene kind")); }
    let mut outputs=Vec::new();
    for (pointer,role) in [("/file","preview"),("/animation/source","animation")] {
        let Some(file)=metadata.pointer(pointer).filter(|file|!file.is_null()).cloned() else {
            if role=="preview" { return Err(Failure::invalid("No preview file returned")); }
            continue;
        };
        let source=(comfy::text(&file,"filename")?.to_string(),file["subfolder"].as_str().unwrap_or("").to_string(),comfy::text(&file,"type")?.to_string());
        let (artifact,stat)=comfy::download_asset(host,&ctx,&source,role)?;
        if stat["kind"]!="file" { return Err(Failure::invalid("Converted 3D output is not a file")); }
        outputs.push(artifact);
        // The companion resolves roles to persisted local assets; no remote URLs reach it.
        *metadata.pointer_mut(pointer).expect("existing pointer")=json!({"assetRole":role,"extension":source.0.rsplit('.').next().unwrap_or("bin")});
    }
    Ok(json!({"kind":"completed","vendorTaskId":null,"outputs":outputs,"data":{"metadataJson":metadata.to_string()},"appliedParams":null,"usage":null}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bgx_adapter_v2_sdk::host;
    struct Converter { calls: usize }
    impl Host for Converter {
        fn call(&mut self, call:bgx_adapter_v2_sdk::HostPortCallV2)->Result<Value,host::HostError> {
            let call=serde_json::to_value(call).unwrap(); self.calls+=1;
            if call["method"]=="host-media.info" {
                return Ok(json!({"id":"preview","kind":"file","mime":"application/octet-stream","byteLength":"32","sha256":"a".repeat(64),"width":null,"height":null,"durationMs":null,"timestampsMs":null,"fileName":"preview.bin"}));
            }
            assert_eq!(call["method"],"host-http.execute");
            let plan=&call["args"]["plan"]; let url=plan["url"].as_str().unwrap();
            let body=if url.ends_with("/upload/image") {
                assert_eq!(plan["body"]["parts"][0]["fileName"],"convert.glb");
                assert_eq!(plan["body"]["parts"][0]["handle"],"owned-source");
                json!({"name":"convert.glb","subfolder":"3d","type":"input"})
            } else if url.ends_with("/panorama_suite/file_3d") {
                let bytes:Vec<u8>=serde_json::from_value(plan["body"]["bytes"].clone()).unwrap();
                let source:Value=serde_json::from_slice(&bytes).unwrap();
                assert_eq!(source,json!({"filename":"convert.glb","subfolder":"3d","storage":"input"}));
                json!({"kind":"mesh","unlit":false,"file":{"filename":"preview.glb","subfolder":"panorama","type":"temp"},"box":{"min":[-1,-1,-1],"max":[1,1,1]}})
            } else {
                assert!(url.ends_with("/view?filename=preview.glb&subfolder=panorama&type=temp"));
                return Ok(json!({"status":200,"headers":[],"payload":{"kind":"media","handle":"derived-file"}}));
            };
            Ok(json!({"status":200,"headers":[],"payload":{"kind":"buffered","bytes":serde_json::to_vec(&body).unwrap()}}))
        }
    }
    #[test]
    fn converts_owned_3d_once_keeps_metadata_and_rejects_paths_before_upload() {
        let mut payload=json!({"operationId":OPERATION,"operationVersion":1,"capabilityKey":CAPABILITY,"modelId":MODEL,
            "input":{"fileName":"room.glb","sources":[{"mediaId":"source"}]},"media":[{"id":"source","kind":"file","handle":"owned-source","mime":"application/octet-stream","fileName":"content.bin","byteLength":"32"}],
            "configuration":{"endpointBaseUrl":"http://127.0.0.1:8188","credentialRef":null}});
        let request=serde_json::from_value(json!({"protocolVersion":2,"operation":"submit","profileId":"binding","capabilitySlot":"generic","requestId":"convert","budgetMs":1000,"payloadJson":payload.to_string(),"bindingId":"binding","negotiatedVersion":2,"enabledFeatures":[]})).unwrap();
        let mut host=Converter{calls:0}; let result=submit(&mut host,&request,&payload).unwrap();
        assert_eq!(host.calls,4);
        serde_json::from_value::<bgx_adapter_v2_sdk::SubmitResult>(result.clone()).unwrap();
        let metadata:Value=serde_json::from_str(result["data"]["metadataJson"].as_str().unwrap()).unwrap();
        assert_eq!(metadata["file"],json!({"assetRole":"preview","extension":"glb"}));
        assert_eq!(metadata["box"]["max"],json!([1,1,1]));
        payload["input"]["fileName"]=json!("../room.glb");
        assert!(submit(&mut host,&request,&payload).is_err()); assert_eq!(host.calls,4);
        payload["input"]["fileName"]=json!("room.glb"); payload["media"]=json!([]);
        assert!(submit(&mut host,&request,&payload).is_err()); assert_eq!(host.calls,4);
        struct Rejected(&'static str);
        impl Host for Rejected {
            fn call(&mut self,_:bgx_adapter_v2_sdk::HostPortCallV2)->Result<Value,host::HostError> {
                Err(serde_json::from_value(json!({"code":self.0,"message":"explicit port refusal"})).unwrap())
            }
        }
        let config=json!({"endpointBaseUrl":"http://127.0.0.1:8188","credentialRef":null});
        let ctx=comfy::Ctx{config:&config,budget_ms:1000};
        let source=json!({"fileName":"scene.glb","handle":"owned-source","mime":"application/octet-stream"});
        for (code,expected) in [("INVALID_INPUT","INVALID_INPUT"),("PERMISSION_DENIED","PERMISSION_DENIED"),("TIMEOUT","ACCEPTANCE_UNKNOWN")] {
            let failure=comfy::upload(&mut Rejected(code),&ctx,&source).unwrap_err();
            assert_eq!(failure.code,expected);
            assert!(failure.message.contains("explicit port refusal"));
        }
    }
}
