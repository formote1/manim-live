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
    pub latest: Option<AnimationSettings>,
    pub latest_theme: Option<String>,
    pub latest_editor: Option<String>,
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

        let qual_re = match Regex::new(r#""quality"\s*:\s*"([^"]+)""#) {
            Ok(r) => r,
            Err(_) => return store,
        };

        let fps_re = match Regex::new(r#""fps"\s*:\s*(\d+)"#) {
            Ok(r) => r,
            Err(_) => return store,
        };

        let theme_re = match Regex::new(r#""theme"\s*:\s*"([^"]+)""#) {
            Ok(r) => r,
            Err(_) => return store,
        };

        let editor_re = match Regex::new(r#""editor"\s*:\s*"([^"]+)""#) {
            Ok(r) => r,
            Err(_) => return store,
        };

        let key_re = match Regex::new(r#""((?:[^"\\]|\\.)+)""#) {
            Ok(r) => r,
            Err(_) => return store,
        };

        // 1. Parse "latest" block if present
        if let Some(latest_idx) = content.find(r#""latest""#) {
            if let Some(open_rel) = content[latest_idx..].find('{') {
                let open_brace = latest_idx + open_rel;
                if let Some(close_rel) = content[open_brace..].find('}') {
                    let latest_body = &content[open_brace + 1..open_brace + close_rel];
                    let quality = qual_re
                        .captures(latest_body)
                        .and_then(|c| c.get(1))
                        .map(|m| match m.as_str().to_lowercase().as_str() {
                            "m" | "medium" | "med" => Quality::Medium,
                            "h" | "high" => Quality::High,
                            "k" | "4k" | "fourk" => Quality::FourK,
                            _ => Quality::Low,
                        });

                    let fps = fps_re
                        .captures(latest_body)
                        .and_then(|c| c.get(1))
                        .and_then(|m| m.as_str().parse::<u32>().ok())
                        .map(|f| f.clamp(1, 120));

                    if let (Some(q), Some(f)) = (quality, fps) {
                        store.latest = Some(AnimationSettings { quality: q, fps: f });
                    } else if let Some(q) = quality {
                        store.latest = Some(AnimationSettings { quality: q, fps: 30 });
                    } else if let Some(f) = fps {
                        store.latest = Some(AnimationSettings { quality: Quality::Low, fps: f });
                    }

                    if let Some(caps) = theme_re.captures(latest_body) {
                        if let Some(m) = caps.get(1) {
                            store.latest_theme = Some(m.as_str().to_string());
                        }
                    }

                    if let Some(caps) = editor_re.captures(latest_body) {
                        if let Some(m) = caps.get(1) {
                            store.latest_editor = Some(m.as_str().to_string());
                        }
                    }
                }
            }
        }

        // 2. Parse "files" block
        let files_idx = match content.find(r#""files""#) {
            Some(i) => i,
            None => return store,
        };
        let open_brace = match content[files_idx..].find('{') {
            Some(i) => files_idx + i,
            None => return store,
        };

        let body = &content[open_brace + 1..];
        let mut depth = 0;
        let mut start_idx = None;
        let mut last_header_start = 0;
        let mut header = "";

        for (i, ch) in body.char_indices() {
            if ch == '{' {
                if depth == 0 {
                    start_idx = Some(i);
                    header = &body[last_header_start..i];
                }
                depth += 1;
            } else if ch == '}' {
                if depth == 1 {
                    if let Some(start) = start_idx {
                        let inner = &body[start + 1..i];
                        if let Some(caps) = key_re.captures_iter(header).last() {
                            if let Some(key_raw) = caps.get(1) {
                                let key = key_raw.as_str().replace(r#"\""#, "\"").replace(r#"\\"#, "\\");
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

                                store.files.insert(key, AnimationSettings { quality, fps });
                            }
                        }
                    }
                    last_header_start = i + 1;
                }
                if depth > 0 {
                    depth -= 1;
                } else {
                    break;
                }
            }
        }

        // If no latest preference was explicitly saved, use the first file preference as fallback
        if store.latest.is_none() && !store.files.is_empty() {
            store.latest = store.files.values().next().copied();
        }

        store
    }

    pub fn get_for_file(&self, path: &Path) -> Option<AnimationSettings> {
        let key = Self::normalize_key(path);
        self.files.get(&key).copied()
    }

    pub fn get_for_file_or_latest(&self, path: &Path) -> Option<AnimationSettings> {
        self.get_for_file(path).or(self.latest)
    }

    pub fn set_for_file(&mut self, path: &Path, quality: Quality, fps: u32) -> Result<()> {
        let key = Self::normalize_key(path);
        self.files.insert(key, AnimationSettings { quality, fps });
        self.latest = Some(AnimationSettings { quality, fps });
        self.save()
    }

    pub fn set_latest_preferences(
        &mut self,
        path: Option<&Path>,
        quality: Quality,
        fps: u32,
        theme: Option<&str>,
        editor: Option<&str>,
    ) -> Result<()> {
        if let Some(p) = path {
            let key = Self::normalize_key(p);
            self.files.insert(key, AnimationSettings { quality, fps });
        }
        self.latest = Some(AnimationSettings { quality, fps });
        if let Some(t) = theme {
            self.latest_theme = Some(t.to_string());
        }
        if let Some(e) = editor {
            self.latest_editor = Some(e.to_string());
        }
        self.save()
    }

    pub fn to_json(&self) -> String {
        let mut json = String::from("{\n");
        if let Some(ref latest) = self.latest {
            let q_str = match latest.quality {
                Quality::Low => "low",
                Quality::Medium => "medium",
                Quality::High => "high",
                Quality::FourK => "4k",
            };
            json.push_str("  \"latest\": {\n");
            json.push_str(&format!("    \"quality\": {:?},\n", q_str));
            json.push_str(&format!("    \"fps\": {}", latest.fps));
            if let Some(ref t) = self.latest_theme {
                json.push_str(&format!(",\n    \"theme\": {:?}", t));
            }
            if let Some(ref e) = self.latest_editor {
                json.push_str(&format!(",\n    \"editor\": {:?}", e));
            }
            json.push_str("\n  },\n");
        }
        json.push_str("  \"files\": {\n");
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

    #[test]
    fn test_corrupted_json_recovery() {
        let corrupted = "{ this is not valid json at all! {[[";
        let parsed = SettingsStore::parse_json(corrupted);
        assert!(parsed.files.is_empty());
    }

    #[test]
    fn test_fps_clamping_and_quality_parsing() {
        let json = r#"{
  "files": {
    "/scene_a.py": {
      "quality": "4K",
      "fps": 500
    },
    "/scene_b.py": {
      "quality": "Low",
      "fps": 0
    }
  }
}"#;
        let parsed = SettingsStore::parse_json(json);
        assert_eq!(
            parsed.files.get("/scene_a.py"),
            Some(&AnimationSettings {
                quality: Quality::FourK,
                fps: 120 // clamped to 120
            })
        );
        assert_eq!(
            parsed.files.get("/scene_b.py"),
            Some(&AnimationSettings {
                quality: Quality::Low,
                fps: 1 // clamped to 1
            })
        );
    }

    #[test]
    fn test_store_remembers_latest_and_defaults_for_new_files() {
        let mut store = SettingsStore::default();
        store.latest = Some(AnimationSettings {
            quality: Quality::High,
            fps: 60,
        });
        store.latest_theme = Some("Gruvbox Dark".to_string());
        store.latest_editor = Some("micro".to_string());

        let json = store.to_json();
        let parsed = SettingsStore::parse_json(&json);

        assert_eq!(
            parsed.latest,
            Some(AnimationSettings {
                quality: Quality::High,
                fps: 60,
            })
        );
        assert_eq!(parsed.latest_theme.as_deref(), Some("Gruvbox Dark"));
        assert_eq!(parsed.latest_editor.as_deref(), Some("micro"));

        // New file gets latest settings
        let new_file = Path::new("/some/new_animation.py");
        assert_eq!(
            parsed.get_for_file_or_latest(new_file),
            Some(AnimationSettings {
                quality: Quality::High,
                fps: 60,
            })
        );
    }
}
