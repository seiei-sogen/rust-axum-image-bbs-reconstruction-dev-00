//! アプリ全体の設定値。
//!
//! 現時点ではページングと画像アップロードの安全なデフォルト値を集約する。

const DEFAULT_PAGE_SIZE: u32 = 20;
const DEFAULT_MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
const DEFAULT_ALLOWED_MIME: &[&str] = &["image/jpeg", "image/gif", "image/png"];

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub page_size: u32,
    pub max_image_bytes: usize,
    pub allowed_mime: &'static [&'static str],
}

impl AppConfig {
    pub fn from_env_or_default() -> Self {
        Self {
            page_size: DEFAULT_PAGE_SIZE,
            max_image_bytes: DEFAULT_MAX_IMAGE_BYTES,
            allowed_mime: DEFAULT_ALLOWED_MIME,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;

    #[test]
    fn default_config_matches_task_limits() {
        let config = AppConfig::from_env_or_default();

        assert_eq!(config.page_size, 20);
        assert_eq!(config.max_image_bytes, 5 * 1024 * 1024);
        assert_eq!(
            config.allowed_mime,
            ["image/jpeg", "image/gif", "image/png"]
        );
    }
}
