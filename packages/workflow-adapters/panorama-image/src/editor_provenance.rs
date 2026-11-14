//! Provenance for reversible preparation. Compare original scene semantics and
//! measured source bytes, while permitting newly imported repair/result assets.
extern crate alloc;
use alloc::{collections::BTreeMap, string::ToString};
use bgx_adapter_v2_sdk::Host;
use serde_json::{json, Value};
use crate::{comfy, mapping::Failure, scene::SceneDocument};

pub fn context(host:&mut dyn Host,payload:&Value,doc:&SceneDocument)->Result<Value,Failure> {
    let sources=doc.sources.iter().map(|s|json!({"nodeId":s.node_id,"inputs":s.inputs,"state":s.document.state})).collect::<alloc::vec::Vec<_>>();
    let mut assets=BTreeMap::new();
    for (node,document) in core::iter::once(("",doc)).chain(doc.sources.iter().map(|s|(s.node_id.as_str(),s.document.as_ref()))) {
        for (name,id) in &document.assets {
            let media=crate::scene_media(payload,id,name)?;
            let stat=comfy::port(host,"host-media.info",json!({"handle":media["handle"]}))?;
            let sha=stat["sha256"].as_str().filter(|s|s.len()==64).ok_or_else(||Failure::invalid("Source digest missing"))?;
            assets.insert(serde_json::to_string(&(node,name)).unwrap(),sha.to_string());
        }
    }
    Ok(json!({"state":doc.state,"inputs":doc.input_node_ids,"sources":sources,"assets":assets}))
}

pub fn require_context(original:&Value,current:&Value)->Result<(),Failure> {
    if !original.is_object() || ["state","inputs","sources"].iter().any(|key|original[*key]!=current[*key]) {
        return Err(Failure::invalid("Source scene or connected scene changed since preparation"));
    }
    let assets=original["assets"].as_object().ok_or_else(||Failure::invalid("Original source identities missing"))?;
    if assets.iter().any(|(name,digest)|current["assets"][name]!=*digest) {
        return Err(Failure::invalid("Original source asset was removed or changed"));
    }
    Ok(())
}

pub fn read_json(host:&mut dyn Host,payload:&Value,doc:&SceneDocument,name:&str)->Result<Value,Failure> {
    let id=doc.assets.get(name).ok_or_else(||Failure::invalid("Provenance is not owned by this document"))?;
    let media=crate::scene_media(payload,id,name)?;
    let stat=comfy::port(host,"host-media.info",json!({"handle":media["handle"]}))?;
    let size=stat["byteLength"].as_str().and_then(|v|v.parse::<usize>().ok()).filter(|n|*n>0 && *n<=1_048_576)
        .ok_or_else(||Failure::invalid("Provenance file is empty or exceeds the document budget"))?;
    let mut bytes=alloc::vec::Vec::with_capacity(size);
    while bytes.len()<size {
        let part=comfy::port(host,"host-media.read",json!({"handle":media["handle"],"offset":bytes.len().to_string(),"maxBytes":(size-bytes.len()).min(65536)}))?;
        let part:alloc::vec::Vec<u8>=serde_json::from_value(part).map_err(|_|Failure::invalid("Invalid managed read"))?;
        if part.is_empty() || part.len()>size-bytes.len() {return Err(Failure::invalid("Incomplete provenance file"));}
        bytes.extend(part);
    }
    serde_json::from_slice(&bytes).map_err(|_|Failure::invalid("Invalid provenance JSON"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repair_allows_new_assets_but_rejects_changed_camera_parents_and_original_bytes() {
        let original=json!({"state":{"camera":{"fov":65}},"inputs":["p"],"sources":[{"nodeId":"p","state":{"yaw":90}}],"assets":{"desk":"original"}});
        let mut current=original.clone();current["assets"]["repair"]=json!("new");
        require_context(&original,&current).unwrap();
        current["assets"]["desk"]=json!("edited");assert!(require_context(&original,&current).is_err());
        current=original.clone();current["state"]["camera"]["fov"]=json!(70);assert!(require_context(&original,&current).is_err());
        current=original.clone();current["sources"][0]["state"]["yaw"]=json!(100);assert!(require_context(&original,&current).is_err());
        current=original.clone();current["inputs"]=json!([]);assert!(require_context(&original,&current).is_err());
    }
}
