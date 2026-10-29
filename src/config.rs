use crate::runner::Quality;
use anyhow::Result;
use regex::Regex;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationSettings {
    pub quality: Quality,
    pub fps: u32,
}

#[derive(Debug, Clone, Default)]
pub struct SettingsStore {
    pub files: HashMap<String, AnimationSettings>,
}

impl SettingsStore {
    pub fn config_file_path() -> Option<PathBuf> {
        let base = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.trim().is_empty() {
                PathBuf::from(xdg)
            } else {
                let home = std::env::var("HOME").ok()?;
                PathBuf::from(home).join(".config")
            }
        } else {
            let home = std::env::var("HOME").ok()?;
            PathBuf::from(home).join(".config")
        };
        Some(base.join("manim-live").join("settings.json"))
    }

    pub fn normalize_key(path: &Path) -> String {
        if let Ok(canon) = path.canonicalize() {
            canon.to_string_lossy().to_string()
        } else if let Ok(cwd) = std::env::current_dir() {
            cwd.join(path).to_string_lossy().to_string()
        } else {
            path.to_string_lossy().to_string()
        }
    }

    pub fn load() -> Self {
        let path = match Self::config_file_path() {
            Some(p) => p,
            None => return Self::default(),
        };

        if !path.exists() {
            return Self::default();
        }

        match fs::read_to_string(&path) {
            Ok(content) => Self::parse_json(&content),
            Err(_) => Self::default(),
        }
    }

    pub fn parse_json(content: &str) -> Self {
        let mut store = Self::default();

        let files_re = match Regex::new(r#""files"\s*:\s*\{([\s\S]*?)\}\s*\}?"#) {
            Ok(r) => r,
            Err(_) => return store,
        };

        let block_re = match Regex::new(r#""((?:[^"\\]|\\.)+)"\s*:\s*\{([^}]+)\}"#) {
            Ok(r) => r,
            Err(_) => return store,
        };

        let qual_re = match Regex::new(r#""quality"\s*:\s*"([^"]+)""#) {
            Ok(r) => r,
            Err(_) => return store,
        };

        let fps_re = match Regex::new(r#""fps"\s*:\s*(\d+)"#) {
            Ok(r) => r,
            Err(_) => return store,
        };

        let files_body = if let Some(caps) = files_re.captures(content) {
            caps.get(1).map(|m| m.as_str()).unwrap_or("")
        } else {
            content
        };

        for block in block_re.captures_iter(files_body) {
            if let Some(key_raw) = block.get(1) {
                let inner = block.get(2).map(|m| m.as_str()).unwrap_or("");
                let quality = qual_re
                    .captures(inner)
                    .and_then(|c| c.get(1))
                    .map(|m| match m.as_str().to_lowercase().as_str() {
                        "m" | "medium" | "med" => Quality::Medium,
                        "h" | "high" => Quality::High,
                        "k" | "4k" | "fourk" => Quality::FourK,
                        _ => Quality::Low,
                    })
                    .unwrap_or(Quality::Low);

                let fps = fps_re
                    .captures(inner)
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<u32>().ok())
                    .unwrap_or(30)
                    .clamp(1, 120);

                let key = key_raw.as_str().replace(r#"\""#, "\"").replace(r#"\\"#, "\\");
                store.files.insert(key, AnimationSettings { quality, fps });
            }
        }

        store
    }

    pub fn get_for_file(&self, path: &Path) -> Option<AnimationSettings> {
        let key = Self::normalize_key(path);
        self.files.get(&key).copied()
    }

    pub fn set_for_file(&mut self, path: &Path, quality: Quality, fps: u32) -> Result<()> {
        let key = Self::normalize_key(path);
        self.files.insert(key, AnimationSettings { quality, fps });
        self.save()
    }

    pub fn to_json(&self) -> String {
        let mut json = String::from("{\n  \"files\": {\n");
        let mut entries: Vec<_> = self.files.iter().collect();
        entries.sort_by_key(|&(k, _)| k);

        for (i, (path, settings)) in entries.iter().enumerate() {
            let comma = if i + 1 < entries.len() { "," } else { "" };
            let q_str = match settings.quality {
                Quality::Low => "low",
                Quality::Medium => "medium",
                Quality::High => "high",
                Quality::FourK => "4k",
            };
            json.push_str(&format!(
                "    {:?}: {{\n      \"quality\": {:?},\n      \"fps\": {}\n    }}{}\n",
                path, q_str, settings.fps, comma
            ));
        }
        json.push_str("  }\n}\n");
        json
    }

    pub fn save(&self) -> Result<()> {
        let target_path = match Self::config_file_path() {
            Some(p) => p,
            None => return Ok(()),
        };

        if let Some(parent) = target_path.parent() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = fs::DirBuilder::new();
                builder.recursive(true);
                builder.mode(0o700);
                let _ = builder.create(parent);
            }
            #[cfg(not(unix))]
            {
                let _ = fs::create_dir_all(parent);
            }
        }

        let json_data = self.to_json();

        let temp_path = target_path.with_extension(format!("tmp.{}", std::process::id()));
        {
            let mut file = File::create(&temp_path)?;
            file.write_all(json_data.as_bytes())?;
            file.flush()?;
            let _ = file.sync_all();
        }

        fs::rename(&temp_path, &target_path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_parse_and_serialize() {
        let mut store = SettingsStore::default();
        store.files.insert(
            "/home/user/anim/scene1.py".to_string(),
            AnimationSettings {
                quality: Quality::High,
                fps: 60,
            },
        );
        store.files.insert(
            "/home/user/anim/scene2.py".to_string(),
            AnimationSettings {
                quality: Quality::Medium,
                fps: 24,
            },
        );

        let json = store.to_json();
        let parsed = SettingsStore::parse_json(&json);

        assert_eq!(
            parsed.files.get("/home/user/anim/scene1.py"),
            Some(&AnimationSettings {
                quality: Quality::High,
                fps: 60
            })
        );
        assert_eq!(
            parsed.files.get("/home/user/anim/scene2.py"),
            Some(&AnimationSettings {
                quality: Quality::Medium,
                fps: 24
            })
        );
    }

    #[test]
    fn test_store_new_file_returns_none() {
        let store = SettingsStore::default();
        let path = Path::new("/never/seen/before/scene.py");
        assert_eq!(store.get_for_file(path), None);
    }
}
