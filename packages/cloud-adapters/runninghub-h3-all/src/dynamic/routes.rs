// Explicit platform routes: never infer a workflow from media count.
#[derive(Clone, Copy)]
pub struct Route {
    pub slug: &'static str,
    pub model: &'static str,
    pub operation: &'static str,
    pub workflow: &'static str,
    pub label: &'static str,
    pub graph: &'static str,
    pub input_schema: &'static str,
    pub output_schema: &'static str,
    pub resume_schema: &'static str,
    pub copy_audio: bool,
}
impl Route {
    pub fn audio_min(&self) -> usize {
        if self.copy_audio {
            1
        } else {
            0
        }
    }
    pub fn audio_max(&self) -> usize {
        if self.copy_audio {
            1
        } else {
            3
        }
    }
    pub fn audio_usage(&self) -> &'static str {
        if self.copy_audio {
            "soundtrack_copy"
        } else {
            "voice_timbre"
        }
    }
}
pub const ROUTES: [Route; 4] = [
    Route {
        slug: "r2v-turbo",
        model: "runninghub-h3-r2v-turbo-dynamic",
        operation: "local.thirdparty.runninghub-h3-all/r2v-turbo",
        workflow: "2100758868111486978",
        label: "多图参考 · Turbo（1–9图）",
        copy_audio: false,
        graph: include_str!("graphs/turbo.json"),
        input_schema: include_str!("../../schemas/dynamic/turbo/input.json"),
        output_schema: include_str!("../../schemas/dynamic/turbo/output.json"),
        resume_schema: include_str!("../../schemas/dynamic/turbo/resume.json"),
    },
    Route {
        slug: "r2v-spectrum",
        model: "runninghub-h3-r2v-spectrum-dynamic",
        operation: "local.thirdparty.runninghub-h3-all/r2v-spectrum",
        workflow: "2116576147646533634",
        label: "多图参考 · Spectrum 均衡（1–9图）",
        copy_audio: false,
        graph: include_str!("graphs/spectrum.json"),
        input_schema: include_str!("../../schemas/dynamic/spectrum/input.json"),
        output_schema: include_str!("../../schemas/dynamic/spectrum/output.json"),
        resume_schema: include_str!("../../schemas/dynamic/spectrum/resume.json"),
    },
    Route {
        slug: "r2v-native",
        model: "runninghub-h3-r2v-dynamic",
        operation: "local.thirdparty.runninghub-h3-all/r2v-native",
        workflow: "2119436868289912833",
        label: "多图参考 · 官方原生（1–9图）",
        copy_audio: false,
        graph: include_str!("graphs/native.json"),
        input_schema: include_str!("../../schemas/dynamic/native/input.json"),
        output_schema: include_str!("../../schemas/dynamic/native/output.json"),
        resume_schema: include_str!("../../schemas/dynamic/native/resume.json"),
    },
    Route {
        slug: "r2v-soundtrack",
        model: "runninghub-h3-r2v-soundtrack",
        operation: "local.thirdparty.runninghub-h3-all/r2v-soundtrack",
        workflow: "2119441622462193666",
        label: "多图参考 · 官方原生·保留原音（1–9图）",
        copy_audio: true,
        graph: include_str!("graphs/soundtrack.json"),
        input_schema: include_str!("../../schemas/dynamic/soundtrack/input.json"),
        output_schema: include_str!("../../schemas/dynamic/soundtrack/output.json"),
        resume_schema: include_str!("../../schemas/dynamic/soundtrack/resume.json"),
    },
];
