//! Read-only metadata for owned external inputs. The Comfy helper calls upstream's own pose/file readers.
extern crate alloc;
use alloc::{format,string::ToString,vec};
use serde_json::{json,Value};
use bgx_adapter_v2_sdk::{Host,Invocation};
use crate::{comfy,mapping::Failure};
pub const OPERATION:&str="local.thirdparty.comfy-panorama-image/editor-input-preview";
const MODEL:&str="panorama-editor-input";
const CAPABILITY:&str="local.thirdparty.comfy-panorama-image/editor-input";
pub fn contract(config:&Value)->Value{
 let input=crate::capability::schema("editor-input-input",&json!({"type":"object","additionalProperties":false,"properties":{
  "sources":{"type":"array","minItems":1,"maxItems":1,"items":{"type":"object","additionalProperties":false,"properties":{"mediaId":{"type":"string","minLength":1}},"required":["mediaId"]}},
  "fileName":{"type":"string","minLength":1},"role":{"type":"string","enum":["background","world","sticker"]},"stateJson":{"type":"string"},"stickerState":{"type":"string"}},"required":["sources","fileName","role","stateJson","stickerState"]}).to_string());
 let output=crate::capability::schema("editor-input-output",&json!({"type":"object","additionalProperties":false,"properties":{"metadataJson":{"type":"string"}},"required":["metadataJson"]}).to_string());
 json!({"schemaVersion":2,"localId":MODEL,"capabilityKey":CAPABILITY,"transportSlot":"generic","modelIds":[MODEL],"configurationSchema":{"id":"config","digest":config["digest"]},
 "operations":[{"id":OPERATION,"version":1,"label":"外部输入预览","inputSchema":{"id":"editor-input-input","digest":input["digest"]},"outputSchema":{"id":"editor-input-output","digest":output["digest"]},"resumeSchema":null,"uiSchema":{"controls":[]},"mediaSlots":[{"role":"sources","inputPointer":"/sources","mediaKind":"file","minCount":1,"maxCount":1}],"execution":{"mode":"synchronous","streaming":false,"cancellation":"unsupported","resumable":false,"remoteIdempotency":false},"outputSlots":[]}],"schemas":[input,output,config]})
}
pub fn submit(host:&mut dyn Host,request:&Invocation,payload:&Value)->Result<Value,Failure>{
 if payload["operationId"]!=OPERATION||payload["operationVersion"]!=1||payload["modelId"]!=MODEL||payload["capabilityKey"]!=CAPABILITY{return Err(Failure::invalid("Input preview operation identity differs"));}
 let input=&payload["input"];let name=comfy::text(input,"fileName")?;
 let extension=name.rsplit_once('.').map(|(_,ext)|ext.to_ascii_lowercase()).unwrap_or_default();
 if name.contains(['/', '\\', ':'])||!["png","jpg","jpeg","webp","bmp","tif","tiff","gif","mp4","webm","mov","mkv","avi","m4v","glb","gltf","obj","stl","fbx","usdz","ply","splat","spz","ksplat"].contains(&extension.as_str()){return Err(Failure::invalid("Unsupported input file"));}
 if !matches!(input["role"].as_str(),Some("background"|"world"|"sticker")){return Err(Failure::invalid("Invalid input role"));}
 let sources=input["sources"].as_array().filter(|v|v.len()==1).ok_or_else(||Failure::invalid("One input file required"))?;
 let mut media=payload["media"].as_array().and_then(|v|v.iter().find(|v|v["id"]==sources[0]["mediaId"])).cloned().ok_or_else(||Failure::invalid("Owned input lease missing"))?;
 media["fileName"]=json!(format!("{}.{extension}",request.r#request_id));
 let ctx=comfy::Ctx{config:&payload["configuration"],budget_ms:request.r#budget_ms};
 let file=comfy::upload(host,&ctx,&media)?;
 let body=json!({"file":file,"role":input["role"],"stateJson":comfy::text(input,"stateJson")?,"stickerState":comfy::text(input,"stickerState")?});
 let result=comfy::exchange(host,&ctx,"POST","/bgx_panorama/input_preview",json!({"kind":"bytes","bytes":serde_json::to_vec(&body).unwrap()}),vec![json!({"name":"Content-Type","value":"application/json"})],false)?;
 Ok(json!({"kind":"completed","vendorTaskId":null,"outputs":[],"data":{"metadataJson":result.to_string()},"appliedParams":null,"usage":null}))
}
