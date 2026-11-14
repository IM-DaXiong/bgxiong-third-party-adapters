//! Model-free repair return and seam preparation. Pixels and pose projection are
//! rendered by the pinned upstream nodes; only validated graph wiring lives here.
extern crate alloc;
use alloc::{collections::BTreeMap, format, string::{String,ToString}, vec::Vec};
use bgx_adapter_v2_sdk::{Host,Invocation};
use serde_json::{json,Value};
use crate::{comfy,editor_provenance as provenance,mapping::Failure,scene::SceneDocument};

const CAP:&str="local.thirdparty.comfy-panorama-image/editor-repair";
pub const MODES:[(&str,&str);3]=[("frame-return","修补图回贴"),("seam-prepare","接缝准备"),("seam-return","接缝还原")];
pub fn mode(payload:&Value)->Option<&'static str> {
    MODES.iter().find(|(mode,_)|payload["operationId"]==format!("{}/editor-{mode}",crate::capability::PLUGIN_ID)
        && payload["modelId"]==format!("panorama-editor-{mode}") && payload["capabilityKey"]==CAP && payload["operationVersion"]==1).map(|x|x.0)
}
pub fn contracts(base:&Value)->Vec<Value> {
    MODES.iter().map(|(mode,label)| {
        let mut c=crate::editor_prepare::contract(base);
        let mut input=crate::workflows::schema("studio-render");
        input["properties"].as_object_mut().unwrap().remove("prompt");
        input["required"].as_array_mut().unwrap().retain(|v|v!="prompt");
        input["properties"]["outputProjection"]=json!({"type":"string","enum":[if *mode=="seam-prepare" {"flat"} else {"equirectangular360"}]});
        input["properties"]["sourceRevision"]=json!({"type":"string","minLength":1,"readOnly":true});
        input["properties"]["sourceName"]=json!({"type":"string","minLength":1});
        input["required"].as_array_mut().unwrap().extend([json!("sourceRevision"),json!("sourceName")]);
        if *mode=="seam-prepare" {
            for (name,min,max,default) in [("bandWidth",1,2048,64),("offset",-2048,2048,0),("blur",0,256,10)] {
                input["properties"][name]=json!({"type":"integer","minimum":min,"maximum":max,"default":default});
                input["required"].as_array_mut().unwrap().push(json!(name));
            }
        } else {
            input["properties"]["provenanceName"]=json!({"type":"string","minLength":1});
            input["required"].as_array_mut().unwrap().push(json!("provenanceName"));
        }
        let input=crate::capability::schema("repair-input",&input.to_string());
        let output=crate::capability::schema("repair-output",&json!({"type":"object","additionalProperties":false,
            "properties":{"mediaArtifacts":serde_json::from_str::<Value>(crate::capability::OUTPUT_SCHEMA).unwrap()["properties"]["mediaArtifacts"],"provenanceJson":{"type":"string","minLength":1}},"required":["mediaArtifacts","provenanceJson"]}).to_string());
        let resume=crate::capability::schema("repair-resume",&json!({"type":"object","additionalProperties":false,
            "properties":{"promptId":{"type":"string","minLength":1},"mode":{"type":"string","enum":[mode]},"provenanceJson":{"type":"string","minLength":1}},"required":["promptId","mode","provenanceJson"]}).to_string());
        let model=format!("panorama-editor-{mode}");c["localId"]=json!(model);c["modelIds"]=json!([model]);c["capabilityKey"]=json!(CAP);
        let op=&mut c["operations"][0];op["id"]=json!(format!("{}/editor-{mode}",crate::capability::PLUGIN_ID));op["label"]=json!(label);
        op["inputSchema"]=json!({"id":"repair-input","digest":input["digest"]});op["outputSchema"]=json!({"id":"repair-output","digest":output["digest"]});op["resumeSchema"]=json!({"id":"repair-resume","digest":resume["digest"]});
        op["outputSlots"]=if *mode=="seam-prepare" {json!([
            {"role":"shifted","kind":"image","minCount":1,"maxCount":1},{"role":"mask","kind":"image","minCount":1,"maxCount":1},
            {"role":"softMask","kind":"image","minCount":1,"maxCount":1},{"role":"provenance","kind":"file","minCount":1,"maxCount":1}])}
            else {json!([{ "role":"repaired","kind":"image","minCount":1,"maxCount":1}])};
        c["schemas"]=json!([input,output,resume,crate::capability::schema("config",crate::capability::CONFIG_SCHEMA)]);c
    }).collect()
}

fn size(stat:&Value)->Result<(u64,u64),Failure> {
    // Scene inputs deliberately use the generic file slot, while the SDK still
    // measures image MIME and dimensions from bytes. Do not confuse slot kind
    // with the detected media type.
    if !matches!(stat["kind"].as_str(),Some("image"|"file")) || !stat["mime"].as_str().is_some_and(|mime|mime.starts_with("image/")) {
        return Err(Failure::invalid("Repair requires a measured image"));
    }
    Ok((stat["width"].as_u64().filter(|n|*n>0).ok_or_else(||Failure::invalid("Image width missing"))?,
        stat["height"].as_u64().filter(|n|*n>0).ok_or_else(||Failure::invalid("Image height missing"))?))
}
/// Exact metadata counterpart of upstream seam_prep.py. Rendering remains upstream.
pub fn seam_shift(width:i64,band:i64,requested:i64)->(i64,i64) {
    let offset=if band>=width {0} else {requested.clamp(band/2-(width+1)/2,width/2-(band+1)/2)};
    (offset,width/2-offset)
}
pub fn graph(mode:&str,input:&Value,doc:&SceneDocument,uploads:&BTreeMap<String,String>,metadata:&Value)->Result<Value,Failure> {
    let name=comfy::text(input,"sourceName")?;
    let id=doc.assets.get(name).ok_or_else(||Failure::invalid("Repair source is not owned"))?;
    let file=uploads.get(id).ok_or_else(||Failure::invalid("Repair source is not uploaded"))?;
    let mut g=json!({"80":{"class_type":"LoadImage","inputs":{"image":file}},"66":{"class_type":"SaveImage","inputs":{"images":["70",0],"filename_prefix":"bgx_panorama_repair"}}});
    if mode=="frame-return" {
        let mp=input["advanced"]["megapixels"].as_f64().ok_or_else(||Failure::invalid("Missing megapixels"))?;
        let multiple=input["advanced"]["multiple"].as_u64().ok_or_else(||Failure::invalid("Missing output multiple"))?;
        let rendered=crate::scene::render_graph(&doc.bind(uploads)?,mp,multiple)?;
        g["70"]=rendered["70"].clone();doc.connect_parents(&mut g,"70",uploads,mp,multiple)?;
        g["71"]=rendered["70"].clone();
        g["71"]["inputs"]["scene"]=json!(["70",0]);g["71"]["inputs"]["sticker"]=json!(["80",0]);
        g["71"]["inputs"]["sticker_state"]=json!(metadata["stickerState"].to_string());
        g["71"]["inputs"]["state_json"]=json!(json!({"version":2,"timeline":doc.state["timeline"],"camera":{"projection":"erp","erp_range":360}}).to_string());
        g["66"]["inputs"]["images"]=json!(["71",1]);
    } else {
        let inverse=mode=="seam-return";
        let offset=metadata["actualOffset"].as_i64().ok_or_else(||Failure::invalid("Actual seam offset missing"))?;
        g["70"]=json!({"class_type":"PanoramaSeamPrep","inputs":{"image":["80",0],
            "seam_width_px":if inverse {json!(1)} else {input["bandWidth"].clone()},
            "seam_center_offset_px":if inverse {-offset} else {offset},"mask_blur_px":if inverse {json!(0)} else {input["blur"].clone()}}});
        if inverse && metadata["startX"]==metadata["width"] {g["66"]["inputs"]["images"]=json!(["80",0]);g.as_object_mut().unwrap().remove("70");}
        if !inverse {
            for (convert,save,index) in [("67","68",1),("69","71",2)] {
                g[convert]=json!({"class_type":"MaskToImage","inputs":{"mask":["70",index]}});
                g[save]=json!({"class_type":"SaveImage","inputs":{"images":[convert,0],"filename_prefix":"bgx_seam_mask"}});
            }
        }
    }
    Ok(g)
}

pub fn submit(host:&mut dyn Host,request:&Invocation,payload:&Value)->Result<Value,Failure> {
    let mode=mode(payload).ok_or_else(||Failure::invalid("Repair operation identity differs"))?;
    let input=&payload["input"];let doc=SceneDocument::parse(comfy::text(input,"editorDocument")?)?;doc.validate_references(input)?;
    let context=provenance::context(host,payload,&doc)?;
    let name=comfy::text(input,"sourceName")?;let id=doc.assets.get(name).ok_or_else(||Failure::invalid("Choose an owned repair source"))?;
    let source=crate::scene_media(payload,id,name)?;let stat=comfy::port(host,"host-media.info",json!({"handle":source["handle"]}))?;let (width,height)=size(&stat)?;
    let metadata=if mode=="seam-prepare" {
        if width!=height*2 {return Err(Failure::invalid("Seam preparation requires a 2:1 ERP image"));}
        let band=input["bandWidth"].as_i64().filter(|v|(1..=2048).contains(v)).ok_or_else(||Failure::invalid("Invalid seam width"))?;
        let offset=input["offset"].as_i64().filter(|v|(-2048..=2048).contains(v)).ok_or_else(||Failure::invalid("Invalid seam offset"))?;
        input["blur"].as_i64().filter(|v|(0..=256).contains(v)).ok_or_else(||Failure::invalid("Invalid seam blur"))?;
        let (actual,start)=seam_shift(width as i64,band,offset);
        json!({"kind":"bgx.panorama.seam","version":1,"sourceRevision":input["sourceRevision"],"sourceContext":context,"sourceName":name,"sourceSha256":stat["sha256"],"width":width,"height":height,"actualOffset":actual,"startX":start})
    } else {
        let meta=provenance::read_json(host,payload,&doc,comfy::text(input,"provenanceName")?)?;
        if meta["kind"]!=if mode=="frame-return" {"bgx.panorama.frame"} else {"bgx.panorama.seam"} || meta["version"]!=1 {return Err(Failure::invalid("Unsupported repair provenance; prepare a new capture without overwriting the old one"));}
        provenance::require_context(&meta["sourceContext"],&context)?;
        let geometry=if mode=="frame-return" {&meta["frame"]} else {&meta};
        if geometry["width"]!=width || geometry["height"]!=height {return Err(Failure::invalid("Repair image dimensions differ from the prepared image"));}
        if mode=="frame-return" && (meta["stickerState"]["version"]!=2 || !meta["stickerState"]["camera"].is_object()) {return Err(Failure::invalid("Original upstream pose is missing"));}
        meta
    };
    if input["outputProjection"]!=if mode=="seam-prepare" {"flat"} else {"equirectangular360"} {return Err(Failure::invalid("Repair output projection differs"));}
    let mut uploads=BTreeMap::new();let mut media=BTreeMap::new();
    for (name,id) in doc.assets.iter().chain(doc.sources.iter().flat_map(|s|s.document.assets.iter())) {
        if media.insert(id.clone(),crate::scene_media(payload,id,name)?).is_some() {return Err(Failure::invalid("Duplicate source lease"));}
        uploads.insert(id.clone(),String::from("pending.png"));
    }
    graph(mode,input,&doc,&uploads,&metadata)?;
    let ctx=comfy::Ctx{config:&payload["configuration"],budget_ms:request.r#budget_ms};
    for (id,source) in media {uploads.insert(id,comfy::upload(host,&ctx,&source)?);}
    let remote=comfy::submit(host,&ctx,&graph(mode,input,&doc,&uploads,&metadata)?,&request.r#request_id)?;
    let result=json!({"kind":"accepted","vendorTaskId":remote,"resumeToken":{"promptId":remote,"mode":mode,"provenanceJson":metadata.to_string()},"appliedParams":input,"usage":null});
    crate::persist_accepted(host,request,&result)?;Ok(result)
}

pub fn query(host:&mut dyn Host,request:&Invocation,payload:&Value)->Result<Value,Failure> {
    let mode=mode(payload).ok_or_else(||Failure::invalid("Repair identity differs"))?;let id=comfy::text(payload,"vendorTaskId")?;
    let token=&payload["resumeToken"];if token["promptId"]!=id || token["mode"]!=mode {return Err(Failure::invalid("Original repair identity differs"));}
    let metadata:Value=serde_json::from_str(comfy::text(token,"provenanceJson")?).map_err(|_|Failure::invalid("Frozen repair provenance missing"))?;
    let ctx=comfy::Ctx{config:&payload["configuration"],budget_ms:request.r#budget_ms};
    let mut result=json!({"status":"running","resumeToken":token,"outputs":[],"data":{"mediaArtifacts":[],"provenanceJson":token["provenanceJson"]},"progress":null,"terminalFailure":null,"usage":null});
    match comfy::status(host,&ctx,id,"66")? {
        comfy::Remote::Queued=>result["status"]=json!("queued"),comfy::Remote::Running=>{},
        comfy::Remote::Lost=>return Err(Failure{code:"RESOURCE_UNAVAILABLE",message:"Original repair unavailable; do not resubmit".into()}),
        comfy::Remote::Failed(code)=>{result["status"]=json!("failed");result["terminalFailure"]=json!({"category":"vendorTaskFailed","message":"Repair rendering failed","vendorCode":code,"details":{"promptId":id}});},
        comfy::Remote::Succeeded(files)=>{
            if files.len()!=1 {return Err(Failure::invalid("Repair expects one image per output"));}
            let mut requested=alloc::vec![(if mode=="seam-prepare" {"shifted"} else {"repaired"},files[0].clone())];
            if mode=="seam-prepare" {
                let outputs=comfy::history_outputs(host,&ctx,id)?;
                for (role,node) in [("mask","68"),("softMask","71")] {
                    let file=outputs[node]["images"].as_array().filter(|a|a.len()==1).map(|a|&a[0]).ok_or_else(||Failure::invalid("Seam mask missing"))?;
                    requested.push((role,(comfy::text(file,"filename")?.into(),file["subfolder"].as_str().unwrap_or("").into(),comfy::text(file,"type")?.into())));
                }
            }
            let mut outputs=Vec::new();let mut identities=Vec::new();
            for (role,file) in requested {
                let (asset,stat)=comfy::download_asset(host,&ctx,&file,role)?;let (w,h)=size(&stat)?;
                if mode!="frame-return" && (metadata["width"]!=w || metadata["height"]!=h) {return Err(Failure::invalid("Seam output changed original dimensions"));}
                let projection=if mode=="seam-prepare" {"flat"} else {if w!=h*2 {return Err(Failure::invalid("Returned panorama is not 2:1"));}"equirectangular360"};
                identities.push(json!({"mediaId":asset["media"]["id"],"role":role,"kind":"image","projection":projection}));outputs.push(asset);
            }
            if mode=="seam-prepare" {outputs.push(comfy::json_asset(host,"provenance",&metadata)?);}
            result["status"]=json!("succeeded");result["outputs"]=json!(outputs);result["data"]["mediaArtifacts"]=json!(identities);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seam_inverse_restores_every_column_at_clamped_boundaries_without_resizing() {
        assert_eq!(size(&json!({"kind":"file","mime":"image/png","width":128,"height":64})).unwrap(),(128,64));
        assert!(size(&json!({"kind":"file","mime":"application/octet-stream","width":128,"height":64})).is_err());
        for width in [2,8,128,4096,8192] {for band in [1,3,64,2048] {for requested in [-2048,-65,0,63,2048] {
            let (offset,start)=seam_shift(width,band,requested);
            let reverse=if start==width {0} else {seam_shift(width,1,-offset).1};
            for x in 0..width {assert_eq!((x+start+reverse)%width,x);}
        }}}
        assert_eq!(seam_shift(128,128,50),(0,64));
        assert_eq!(seam_shift(128,3,2048),(62,2));
    }
    #[test]
    fn frame_return_passes_exact_upstream_pose_and_original_scene_to_new_erp_output() {
        let doc=SceneDocument::parse(&json!({"stateText":json!({"version":2,"camera":{"projection":"perspective"},"timeline":{"fps":29.97,"length":121}}).to_string(),"assets":[{"name":"repair","mediaId":"m"}]}).to_string()).unwrap();
        let pose=json!({"stickerState":{"version":2,"fps":29.97,"camera":{"rotation":[{"frame":121,"value":{"yaw_deg":-90}}]}}});
        let g=graph("frame-return",&json!({"sourceName":"repair","advanced":{"megapixels":2,"multiple":8}}),&doc,&BTreeMap::from([("m".into(),"repair.png".into())]),&pose).unwrap();
        assert_eq!(g["71"]["inputs"]["scene"],json!(["70",0]));assert_eq!(g["71"]["inputs"]["sticker"],json!(["80",0]));
        assert_eq!(g["71"]["inputs"]["sticker_state"],pose["stickerState"].to_string());assert_eq!(g["66"]["inputs"]["images"],json!(["71",1]));
    }
}
