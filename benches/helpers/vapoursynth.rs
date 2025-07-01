use av_decoders::DecoderError;
use vapoursynth::{api::API, core::CoreRef, format::PresetFormat, node::Node, plugin::Plugin};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[allow(dead_code)]
enum PluginId {
    Std,
    Resize,
    Lsmash,
    Ffms2,
    BestSource,
    DGDecNV,
    Julek,
    Vszip,
    Vship,
}

impl PluginId {
    const fn as_str(self) -> &'static str {
        match self {
            PluginId::Std => "com.vapoursynth.std",
            PluginId::Resize => "com.vapoursynth.resize",
            PluginId::Lsmash => "systems.innocent.lsmas",
            PluginId::Ffms2 => "com.vapoursynth.ffms2",
            PluginId::BestSource => "com.vapoursynth.bestsource",
            PluginId::DGDecNV => "com.vapoursynth.dgdecodenv",
            PluginId::Julek => "com.julek.plugin",
            PluginId::Vszip => "com.julek.vszip",
            PluginId::Vship => "com.lumen.vship",
        }
    }
}

fn get_plugin(core: CoreRef, plugin_id: PluginId) -> Result<Plugin, DecoderError> {
    let err_msg = || {
        format!(
            "Failed to get VapourSynth {plugin_id} plugin",
            plugin_id = plugin_id.as_str()
        )
    };
    let plugin = core
        .get_plugin_by_id(plugin_id.as_str())
        .map_err(|_e| DecoderError::VapoursynthInternalError { cause: err_msg() })
        .unwrap()
        .ok_or(|| DecoderError::VapoursynthInternalError { cause: err_msg() })
        .map_err(|_e| DecoderError::VapoursynthInternalError { cause: err_msg() })
        .unwrap();

    Ok(plugin)
}

pub(crate) fn resize_node<'core>(
    core: CoreRef<'core>,
    node: &Node<'core>,
    width: Option<u32>,
    height: Option<u32>,
    format: Option<PresetFormat>,
    matrix_in_s: Option<&'static str>,
) -> Result<Node<'core>, DecoderError> {
    let api = API::get()
        .ok_or(DecoderError::VapoursynthInternalError {
            cause: "Failed to get VapourSynth API".to_owned(),
        })
        .unwrap();
    let std = get_plugin(core, PluginId::Resize).unwrap();

    let error_message = || {
        format!(
            "Failed to resize video to {width}x{height}",
            width = width.unwrap_or(0),
            height = height.unwrap_or(0)
        )
    };

    let mut arguments = vapoursynth::map::OwnedMap::new(api);
    arguments
        .set("clip", node)
        .map_err(|_| DecoderError::VapoursynthArgsError {
            cause: error_message(),
        })
        .unwrap();
    if let Some(width) = width {
        arguments
            .set_int("width", width as i64)
            .map_err(|_| DecoderError::VapoursynthArgsError {
                cause: error_message(),
            })
            .unwrap();
    }
    if let Some(height) = height {
        arguments
            .set_int("height", height as i64)
            .map_err(|_| DecoderError::VapoursynthArgsError {
                cause: error_message(),
            })
            .unwrap();
    }
    if let Some(format) = format {
        arguments
            .set_int("format", format as i64)
            .map_err(|_| DecoderError::VapoursynthArgsError {
                cause: error_message(),
            })
            .unwrap();
    }
    if let Some(matrix_in_s) = matrix_in_s {
        arguments
            .set("matrix_in_s", &matrix_in_s.as_bytes())
            .map_err(|_| DecoderError::VapoursynthArgsError {
                cause: error_message(),
            })
            .unwrap();
    }

    std.invoke("Bicubic", &arguments)
        .map_err(|_| DecoderError::VapoursynthInternalError {
            cause: error_message(),
        })
        .unwrap()
        .get_node("clip")
        .map_err(|_| DecoderError::VapoursynthInternalError {
            cause: error_message(),
        })
}
