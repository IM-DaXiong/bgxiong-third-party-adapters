use super::*;

pub(super) const MODEL_FIELD: &str = "/freeze/userParams/samModel";
const PREFERRED: &str = "sam_vit_b_01ec64.pth";
const EMPTY: &str = "SAM 加载器没有列出可用权重。请将 SAM 模型放入 ComfyUI 的 models/sams 目录并刷新模型列表。";

fn choices(info: &Value) -> Result<Vec<String>> {
    if !info["SAMLoader"].is_object() {
        return Err(fail("RESOURCE_UNAVAILABLE", "当前 ComfyUI 未注册 SAMLoader。请安装或启用 ComfyUI-Impact-Pack 后重启 ComfyUI。"));
    }
    let spec = &info["SAMLoader"]["input"]["required"]["model_name"];
    let listed = spec[0].as_array().or_else(|| {
        (spec[0] == "COMBO").then(|| spec[1]["options"].as_array()).flatten()
    }).ok_or_else(|| fail("RESOURCE_UNAVAILABLE", "当前 SAMLoader 未提供可读取的模型列表，请检查节点版本。"))?;
    let mut names: Vec<_> = listed.iter().filter_map(Value::as_str).filter(|name| {
        *name == "ESAM" || [".pt", ".pth", ".safetensors"].iter().any(|ext| name.to_ascii_lowercase().ends_with(ext))
    }).map(str::to_owned).collect();
    names.sort();
    names.dedup();
    if names.is_empty() { return Err(fail("RESOURCE_UNAVAILABLE", EMPTY)); }
    Ok(names)
}

fn discover(host: &mut dyn Host, base: &str) -> Result<Vec<String>> {
    let base = base.trim_end_matches('/');
    let read = |host: &mut dyn Host, class: &str| {
        http_json(host, "GET", &format!("{base}/object_info/{class}"), None)
            .map_err(|error| fail(error.code, format!("读取 {class} 失败，请检查当前 ComfyUI 连接并重试。")))
    };
    let models = choices(&read(host, "SAMLoader")?)?;
    if !read(host, "H3FaceMaskSAM")?["H3FaceMaskSAM"].is_object() {
        return Err(fail("RESOURCE_UNAVAILABLE", "当前 ComfyUI 未注册 H3FaceMaskSAM，请加载人脸精修配套节点并重启 ComfyUI。"));
    }
    Ok(models)
}

pub(super) fn probe(host: &mut dyn Host, payload: &Value) -> Result<Value> {
    if payload["operationId"] != OP_REFINE || payload["modelId"] != MODEL {
        return Err(invalid("该操作没有动态表单选项"));
    }
    let base = cfg_base(&payload["configuration"])?;
    let field = match discover(host, base) {
        Ok(names) => json!({"fieldPointer": MODEL_FIELD, "choices": names}),
        Err(error) => json!({"fieldPointer": MODEL_FIELD, "choices": [], "reason": error.message}),
    };
    Ok(json!({"kind":"operation_form_options","fields":[field]}))
}

pub(super) fn resolve_for_submit(host: &mut dyn Host, base: &str, params: &mut Value) -> Result<()> {
    if params["samEnabled"] != true { return Ok(()); }
    let available = discover(host, base)?;
    let selected = if let Some(explicit) = params["samModel"].as_str().filter(|s| !s.trim().is_empty()) {
        available.iter().find(|s| s.as_str() == explicit).ok_or_else(|| {
            fail("RESOURCE_UNAVAILABLE", "此前选择的 SAM 模型已不在当前 ComfyUI，请刷新并重新选择。")
        })?
    } else {
        available.iter().find(|s| s.rsplit(['/', '\\']).next() == Some(PREFERRED))
            .or_else(|| available.iter().find(|s| s.as_str() != "ESAM"))
            .unwrap_or(&available[0])
    };
    params["samModel"] = json!(selected);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Server { models: Value, mask: bool, requests: usize }
    impl Host for Server {
        fn call(&mut self, call: bgx_adapter_v2_sdk::HostPortCallV2) -> core::result::Result<Value, host::HostError> {
            let call = serde_json::to_value(call).unwrap();
            let plan = &call["args"]["plan"];
            assert_eq!(plan["method"], "GET");
            self.requests += 1;
            let body = match plan["url"].as_str().unwrap() {
                "http://localhost/object_info/SAMLoader" => self.models.clone(),
                "http://localhost/object_info/H3FaceMaskSAM" if self.mask => json!({"H3FaceMaskSAM":{}}),
                "http://localhost/object_info/H3FaceMaskSAM" => json!({}),
                _ => panic!("unexpected route"),
            };
            Ok(json!({"status":200,"payload":{"bytes":serde_json::to_vec(&body).unwrap()}}))
        }
    }
    fn server(spec: Value) -> Server {
        Server { models:json!({"SAMLoader":{"input":{"required":{"model_name":spec}}}}), mask:true, requests:0 }
    }

    #[test]
    fn same_inventory_drives_form_and_workflow() {
        let mut srv = server(json!(["COMBO",{"options":["ESAM","a.pt",PREFERRED,"None","a.pt"]}]));
        let response = probe(&mut srv,&json!({"operationId":OP_REFINE,"modelId":MODEL,"configuration":{"endpointBaseUrl":"http://localhost"}})).unwrap();
        assert_eq!(response["fields"][0]["choices"],json!(["ESAM","a.pt",PREFERRED]));
        for (requested, expected) in [(Value::Null,PREFERRED),(json!("a.pt"),"a.pt"),(json!("ESAM"),"ESAM")] {
            let mut params = json!({"samEnabled":true,"samModel":requested,"appearanceImages":[]});
            resolve_for_submit(&mut srv,"http://localhost",&mut params).unwrap();
            let graph = inject_refine_workflow("", "test.mp4", "", &params).unwrap();
            assert_eq!(graph["sam_loader"]["inputs"]["model_name"],expected);
            assert_eq!(graph["stitch"]["inputs"]["masks"],json!(["sam_mask",0]));
        }
        assert!(resolve_for_submit(&mut srv,"http://localhost",&mut json!({"samEnabled":true,"samModel":"gone.pt"})).is_err());
    }

    #[test]
    fn missing_resources_fail_before_generation_and_off_does_no_io() {
        let mut srv = server(json!([["None"]]));
        resolve_for_submit(&mut srv,"http://localhost",&mut json!({"samEnabled":false})).unwrap();
        assert_eq!(srv.requests,0);
        assert_eq!(discover(&mut srv,"http://localhost").unwrap_err().message, EMPTY);
        srv.models=json!({});
        assert!(discover(&mut srv,"http://localhost").unwrap_err().message.contains("Impact-Pack"));
        srv=server(json!([[PREFERRED]]));
        srv.mask=false;
        assert!(discover(&mut srv,"http://localhost").unwrap_err().message.contains("H3FaceMaskSAM"));
    }
}
