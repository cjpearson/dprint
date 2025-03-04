use anyhow::bail;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use dprint_core::plugins::PluginInfo;

use super::process;
use super::wasm;
use crate::environment::Environment;
use crate::plugins::Plugin;
use crate::plugins::PluginCache;
use crate::plugins::PluginSourceReference;
use crate::plugins::PluginsCollection;
use crate::utils::PathSource;
use crate::utils::PluginKind;

pub struct SetupPluginResult {
  pub file_path: PathBuf,
  pub plugin_info: PluginInfo,
}

pub async fn setup_plugin<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  file_bytes: &[u8],
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  match url_or_file_path.plugin_kind() {
    Some(PluginKind::Wasm) => wasm::setup_wasm_plugin(url_or_file_path, file_bytes, environment),
    Some(PluginKind::Process) => process::setup_process_plugin(url_or_file_path, file_bytes, environment).await,
    None => {
      bail!("Could not resolve plugin type from url or file path: {}", url_or_file_path.display());
    }
  }
}

pub fn get_file_path_from_plugin_info<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  plugin_info: &PluginInfo,
  environment: &TEnvironment,
) -> Result<PathBuf> {
  match url_or_file_path.plugin_kind() {
    Some(PluginKind::Wasm) => Ok(wasm::get_file_path_from_plugin_info(plugin_info, environment)),
    Some(PluginKind::Process) => Ok(process::get_file_path_from_plugin_info(plugin_info, environment)),
    None => {
      bail!("Could not resolve plugin type from url or file path: {}", url_or_file_path.display());
    }
  }
}

/// Deletes the plugin from the cache.
pub fn cleanup_plugin<TEnvironment: Environment>(url_or_file_path: &PathSource, plugin_info: &PluginInfo, environment: &TEnvironment) -> Result<()> {
  match url_or_file_path.plugin_kind() {
    Some(PluginKind::Wasm) => wasm::cleanup_wasm_plugin(plugin_info, environment),
    Some(PluginKind::Process) => process::cleanup_process_plugin(plugin_info, environment),
    None => {
      bail!("Could not resolve plugin type from url or file path: {}", url_or_file_path.display());
    }
  }
}

pub async fn create_plugin<TEnvironment: Environment>(
  plugins_collection: Arc<PluginsCollection<TEnvironment>>,
  plugin_cache: &PluginCache<TEnvironment>,
  environment: TEnvironment,
  plugin_reference: &PluginSourceReference,
) -> Result<Box<dyn Plugin>> {
  let cache_item = plugin_cache.get_plugin_cache_item(plugin_reference).await;
  let cache_item = match cache_item {
    Ok(cache_item) => Ok(cache_item),
    Err(err) => {
      log_verbose!(
        environment,
        "Error getting plugin from cache. Forgetting from cache and retrying. Message: {}",
        err.to_string()
      );

      // forget and try again
      plugin_cache.forget(plugin_reference)?;
      plugin_cache.get_plugin_cache_item(plugin_reference).await
    }
  }?;

  match plugin_reference.plugin_kind() {
    Some(PluginKind::Wasm) => {
      let file_bytes = match environment.read_file_bytes(cache_item.file_path) {
        Ok(file_bytes) => file_bytes,
        Err(err) => {
          log_verbose!(
            environment,
            "Error reading plugin file bytes. Forgetting from cache and retrying. Message: {}",
            err.to_string()
          );

          // forget and try again
          plugin_cache.forget(plugin_reference)?;
          let cache_item = plugin_cache.get_plugin_cache_item(plugin_reference).await?;
          environment.read_file_bytes(cache_item.file_path)?
        }
      };

      Ok(Box::new(wasm::WasmPlugin::new(&file_bytes, cache_item.info, environment, plugins_collection)?))
    }
    Some(PluginKind::Process) => {
      let cache_item = if !environment.path_exists(&cache_item.file_path) {
        log_verbose!(
          environment,
          "Could not find process plugin at {}. Forgetting from cache and retrying.",
          cache_item.file_path.display()
        );

        // forget and try again
        plugin_cache.forget(plugin_reference)?;
        plugin_cache.get_plugin_cache_item(plugin_reference).await?
      } else {
        cache_item
      };

      let executable_path = super::process::get_test_safe_executable_path(cache_item.file_path, &environment);
      Ok(Box::new(process::ProcessPlugin::new(
        environment,
        executable_path,
        cache_item.info,
        plugins_collection,
      )))
    }
    None => {
      bail!("Could not resolve plugin type from url or file path: {}", plugin_reference.display());
    }
  }
}
