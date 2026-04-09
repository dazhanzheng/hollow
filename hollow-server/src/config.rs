pub struct Config {
    pub port: u16,
    pub log_level: String,
}

impl Config {
    pub fn from_env() -> Self {
        Config {
            port: std::env::var("HOLLOW_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3000),
            log_level: std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        // SAFETY: test-only, single-threaded context
        unsafe { std::env::remove_var("HOLLOW_PORT") };
        let config = Config::from_env();
        assert_eq!(config.port, 3000);
        assert!(!config.log_level.is_empty());
    }
}
