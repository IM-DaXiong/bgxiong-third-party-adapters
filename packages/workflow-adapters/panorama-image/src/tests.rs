//! Local protocol/mapping tests. No live ComfyUI.
use serde_json::json;

use crate::capability::{capabilities, CAPABILITY_KEY, FEATURE_PROJECTION, MODEL_ID, OPERATION_ID};
use crate::mapping::{build_graph, reference_id};

fn valid_input() -> serde_json::Value {
    json!({
        "prompt": "a bright living room with windows",
        "outputProjection": "equirectangular360",
        "references": [{"mediaId": "img-1"}],
        "width": 2048,
        "height": 1024,
        "seed": 7,
        "advanced": {"steps": 20}
    })
}

#[test]
fn capabilities_declare_projection_feature_contract() {
    let caps = capabilities();
    assert_eq!(caps["pluginId"], "local.thirdparty.comfy-panorama-image");
    let c = &caps["capabilityContracts"][0];
    assert_eq!(c["capabilityKey"], CAPABILITY_KEY);
    assert_eq!(c["modelIds"][0], MODEL_ID);
    assert_eq!(c["operations"][0]["id"], OPERATION_ID);
    assert_eq!(c["operations"][0]["mediaSlots"][0]["maxCount"], 1);
    assert_eq!(c["operations"][0]["mediaSlots"][0]["minCount"], 1);
    // Feature is on the package manifest; capability payload still uses image.generate.
    assert_eq!(FEATURE_PROJECTION, "media-projection-v1");
}

#[test]
fn mapping_accepts_single_reference_and_binds_upload() {
    let (graph, applied) = build_graph(&valid_input(), "uploaded.png").expect("ok");
    assert_eq!(graph["100"]["inputs"]["image"], "uploaded.png");
    assert_eq!(graph["31"]["inputs"]["seed"], 7);
    assert_eq!(graph["31"]["inputs"]["steps"], 20);
    assert_eq!(applied["outputProjection"], "equirectangular360");
    let input = valid_input();
    assert!(applied.as_object().unwrap().keys().all(|key| input.get(key).is_some()));
    assert_eq!(applied["advanced"], input["advanced"]);
    assert_eq!(applied["prompt"], graph["6"]["inputs"]["text"]);
    let state = graph["70"]["inputs"]["sticker_state"].as_str().unwrap();
    assert!(state.contains("\"hFOV_deg\":90"));
    assert!(state.contains("\"yaw_deg\":0"));
}

#[test]
fn mapping_rejects_wrong_cardinality_projection_and_size() {
    let mut zero = valid_input();
    zero["references"] = json!([]);
    assert!(build_graph(&zero, "x.png").is_err());

    let mut two = valid_input();
    two["references"] = json!([{"mediaId": "a"}, {"mediaId": "b"}]);
    assert!(build_graph(&two, "x.png").is_err());

    let mut flat = valid_input();
    flat["outputProjection"] = json!("flat");
    assert!(build_graph(&flat, "x.png").is_err());

    let mut size = valid_input();
    size["width"] = json!(1024);
    assert!(build_graph(&size, "x.png").is_err());
}

#[test]
fn reference_id_reads_single_slot() {
    assert_eq!(reference_id(&valid_input()).unwrap(), "img-1");
}

#[test]
fn manifest_feature_constant_matches_plan() {
    let raw = include_str!("../manifest.json");
    assert!(raw.contains("media-projection-v1"));
    assert!(raw.contains("local.thirdparty.comfy-panorama-image"));
    assert!(raw.contains("allowPrivateNetwork"));
}

#[test]
fn scene_submission_uploads_all_views_and_checkpoints_one_qwen_prompt() {
    use bgx_adapter_v2_sdk::{Host, host};
    use serde_json::Value;
    struct SceneHost { uploads: usize, prompts: usize, checkpointed: bool }
    impl Host for SceneHost {
        fn call(&mut self, call:bgx_adapter_v2_sdk::HostPortCallV2)->Result<Value,host::HostError> {
            let call=serde_json::to_value(call).unwrap();
            match call["method"].as_str().unwrap() {
                "host-checkpoint.read" => return Ok(json!({"revision":"1","valueJson":"{\"revision\":\"1\"}"})),
                "host-checkpoint.write" => {
                    let saved:Value=serde_json::from_str(call["args"]["valueJson"].as_str().unwrap()).unwrap();
                    assert_eq!(saved["vendorTaskId"], "one-scene-task");
                    assert_eq!(saved["resumeToken"]["promptId"], "one-scene-task");
                    self.checkpointed=true;
                    return Ok(json!({"revision":"2"}));
                },
                "host-http.execute" => {},
                other => panic!("unexpected port {other}"),
            }
            let plan=&call["args"]["plan"];
            let url=plan["url"].as_str().unwrap();
            let body=if url.ends_with("/upload/image") {
                self.uploads+=1;
                assert_eq!(plan["body"]["parts"][0]["handle"], format!("lease-{}",self.uploads));
                let extension=["png","mp4","glb"][self.uploads-1];
                assert!(plan["body"]["parts"][0]["fileName"].as_str().unwrap().ends_with(extension));
                json!({"name":format!("view-{}.{extension}",self.uploads),"subfolder":"frozen","type":"input"})
            } else {
                assert!(url.ends_with("/prompt")); self.prompts+=1;
                assert_eq!(self.uploads,3);
                let bytes:Vec<u8>=serde_json::from_value(plan["body"]["bytes"].clone()).unwrap();
                let graph:Value=serde_json::from_slice::<Value>(&bytes).unwrap()["prompt"].clone();
                let state:Value=serde_json::from_str(graph["83"]["inputs"]["state_json"].as_str().unwrap()).unwrap();
                for (i,yaw) in [0,-90,180].iter().enumerate() {
                    assert_eq!(state["stickers"][i]["direction"]["value"]["yaw_deg"],*yaw);
                    assert_eq!(state["stickers"][i]["source"]["filename"],format!("view-{}.{}",i+1,["png","mp4","glb"][i]));
                }
                assert_eq!(graph["64"]["inputs"]["images.image_1"],json!(["83",1]));
                assert_eq!(graph["3"]["inputs"]["seed"],7);
                assert_eq!(graph["37"]["inputs"]["unet_name"],"qwen_image_2.1_int8_convrot.safetensors");
                json!({"prompt_id":"one-scene-task","node_errors":{}})
            };
            Ok(json!({"status":200,"headers":[],"payload":{"kind":"buffered","bytes":serde_json::to_vec(&body).unwrap()}}))
        }
    }
    let mut input=valid_input();
    let mut assets=Vec::new(); let mut stickers=Vec::new(); let mut media=Vec::new(); let mut refs=Vec::new();
    for (i,yaw) in [0,-90,180].iter().enumerate() {
        let id=format!("image-{}",i+1); let filename=format!("{}.{}",i+1,["png","mp4","glb"][i]);
        assets.push(json!({"name":format!("input//{filename}"),"mediaId":id}));
        refs.push(json!({"mediaId":id}));
        let mime=["image/png","video/mp4","application/octet-stream"][i];
        media.push(json!({"id":id,"kind":"file","fileName":if i==2 {"opaque.bin"} else {&filename},"handle":format!("lease-{}",i+1),"mime":mime}));
        stickers.push(json!({"id":id,"source":{"kind":"file","filename":filename,"storage":"input"},"direction":{"value":{"yaw_deg":yaw}}}));
    }
    let state=json!({"version":2,"camera":{"projection":"erp","erp_range":360},"stickers":stickers});
    input["references"]=json!(refs);
    input["editorDocument"]=json!(json!({"stateText":state.to_string(),"assets":assets}).to_string());
    let mut payload=json!({"modelId":crate::capability::SCENE_MODEL,"capabilityKey":CAPABILITY_KEY,"operationId":crate::capability::SCENE_OPERATION,"operationVersion":1,"input":input,"media":media,"configuration":{"endpointBaseUrl":"http://127.0.0.1:8188","credentialRef":null}});
    let invocation=serde_json::from_value(json!({"protocolVersion":2,"operation":"submit","profileId":"binding","capabilitySlot":"image","requestId":"scene","budgetMs":1000,"payloadJson":payload.to_string(),"bindingId":"binding","negotiatedVersion":2,"enabledFeatures":[]})).unwrap();
    let mut host=SceneHost{uploads:0,prompts:0,checkpointed:false};
    let accepted=crate::submit(&mut host,&invocation,&payload).unwrap();
    assert_eq!(accepted["vendorTaskId"],"one-scene-task");
    assert_eq!(host.prompts,1); assert!(host.checkpointed);
    payload["media"].as_array_mut().unwrap().pop();
    let mut refused=SceneHost{uploads:0,prompts:0,checkpointed:false};
    assert!(crate::submit(&mut refused,&invocation,&payload).is_err());
    assert_eq!(refused.uploads,0); assert_eq!(refused.prompts,0);
}

#[test]
fn query_uses_public_query_dto_and_only_the_original_prompt() {
    use bgx_adapter_v2_sdk::{Host, host};
    struct QueueHost { complete: bool, calls: usize }
    impl Host for QueueHost {
        fn call(&mut self, c:bgx_adapter_v2_sdk::HostPortCallV2)->Result<serde_json::Value,host::HostError> {
            let c=serde_json::to_value(c).unwrap();
            self.calls += 1;
            if c["method"] == "host-media.info" {
                assert_eq!(c["args"]["handle"], "lease-result");
                return Ok(json!({"id":"result-id","kind":"image","mime":"image/jpeg","byteLength":"100","sha256":"a".repeat(64),"width":2048,"height":1024,"durationMs":null,"timestampsMs":null,"fileName":"original.jpg"}));
            }
            assert_eq!(c["method"],"host-http.execute");
            assert_eq!(c["args"]["plan"]["method"],"GET");
            let url=c["args"]["plan"]["url"].as_str().unwrap();
            if url.contains("/view?") {
                assert!(self.complete);
                assert_eq!(c["args"]["plan"]["sink"]["kind"], "media");
                return Ok(json!({"status":200,"headers":[],"payload":{"kind":"media","handle":"lease-result"}}));
            }
            let body=if url.ends_with("/queue") {json!({"queue_running":[[1,"original",{},{}]],"queue_pending":[]})}
                else {assert!(url.ends_with("/history/original")); if self.complete {json!({"original":{"status":{"status_str":"success"},"outputs":{"66":{"images":[{"filename":"original.jpg","subfolder":"","type":"output"}]}}}})} else {json!({})}};
            Ok(json!({"status":200,"headers":[],"payload":{"kind":"buffered","bytes":serde_json::to_vec(&body).unwrap()}}))
        }
    }
    let payload=json!({"modelId":MODEL_ID,"capabilityKey":CAPABILITY_KEY,"operationId":OPERATION_ID,"operationVersion":1,"vendorTaskId":"original","resumeToken":{"promptId":"original"},"configuration":{"endpointBaseUrl":"http://127.0.0.1:8188","credentialRef":null}});
    let invocation=serde_json::from_value(json!({"protocolVersion":2,"operation":"query","profileId":"binding","capabilitySlot":"image","requestId":"q","budgetMs":1000,"payloadJson":payload.to_string(),"bindingId":"binding","negotiatedVersion":2,"enabledFeatures":[]})).unwrap();
    let result=crate::query(&mut QueueHost {complete:false,calls:0},&invocation,&payload).unwrap();
    assert_eq!(result["status"],"running");
    serde_json::from_value::<bgx_adapter_v2_sdk::QueryResult>(result).unwrap();
    let mut h=QueueHost {complete:true,calls:0};
    let result=crate::query(&mut h,&invocation,&payload).unwrap();
    assert_eq!(h.calls,4);
    assert_eq!(result["status"],"succeeded");
    assert_eq!(result["data"]["mediaArtifacts"][0]["mediaId"],"result-id");
    assert_eq!(result["outputs"][0]["media"]["mime"],"image/jpeg");
    serde_json::from_value::<bgx_adapter_v2_sdk::QueryResult>(result).unwrap();
}
