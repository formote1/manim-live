use crossbeam_channel::Sender;
use regex::Regex;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Low,
    Medium,
    High,
    FourK,
}

impl Quality {
    pub fn to_flag(self) -> &'static str {
        match self {
            Quality::Low => "-ql",
            Quality::Medium => "-qm",
            Quality::High => "-qh",
            Quality::FourK => "-qk",
        }
    }

    #[allow(dead_code)]
    pub fn rank(self) -> u8 {
        match self {
            Quality::Low => 1,
            Quality::Medium => 2,
            Quality::High => 3,
            Quality::FourK => 4,
        }
    }

    #[allow(dead_code)]
    pub fn clamp_to_cap(self, cap: Quality) -> Quality {
        if self.rank() > cap.rank() {
            cap
        } else {
            self
        }
    }

    pub fn max_height(self) -> u32 {
        match self {
            Quality::Low => 480,
            Quality::Medium => 720,
            Quality::High => 1080,
            Quality::FourK => 2160,
        }
    }
}


#[derive(Debug, Clone)]
pub struct FrameArtifacts {
    pub frames: Vec<PathBuf>,
    pub temp_dir_guard: Option<Arc<tempfile::TempDir>>,
}


#[derive(Debug, Clone)]
pub enum BuildEvent {
    Started { command: String },
    OutputLine(String),
    Progress { percentage: f32, message: String },
    Finished {
        success: bool,
        duration: Duration,
        output_path: Option<PathBuf>,
        artifacts: FrameArtifacts,
    },
}

pub struct BuildRunner {
    sender: Sender<BuildEvent>,
    current_child: Arc<Mutex<Option<Child>>>,
    current_cancel: Arc<Mutex<Option<Arc<AtomicBool>>>>,
}

#[derive(Debug, Clone)]
pub struct ResolvedManim {
    pub bin_path: PathBuf,
    pub venv_dir: Option<PathBuf>,
}

pub fn resolve_manim(explicit: Option<&str>, target_file: &Path) -> Option<ResolvedManim> {
    if let Some(exp) = explicit {
        if !exp.trim().is_empty() {
            let p = PathBuf::from(exp);
            if p.is_file() {
                let venv = p.parent().and_then(|bin| bin.parent()).map(|v| v.to_path_buf());
                return Some(ResolvedManim {
                    bin_path: p,
                    venv_dir: venv,
                });
            }
            if let Ok(output) = Command::new("which").arg(exp).output() {
                if output.status.success() {
                    let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path_str.is_empty() {
                        return Some(ResolvedManim {
                            bin_path: PathBuf::from(path_str),
                            venv_dir: None,
                        });
                    }
                }
            }
        }
    }

    if let Ok(env_bin) = std::env::var("MANIM_BIN") {
        if !env_bin.trim().is_empty() {
            let p = PathBuf::from(&env_bin);
            if p.is_file() {
                let venv = p.parent().and_then(|bin| bin.parent()).map(|v| v.to_path_buf());
                return Some(ResolvedManim {
                    bin_path: p,
                    venv_dir: venv,
                });
            }
        }
    }

    // Check target file parent directories for virtualenvs
    let canonical = target_file.canonicalize().unwrap_or_else(|_| target_file.to_path_buf());
    let mut current = canonical.parent();
    while let Some(dir) = current {
        for venv_name in &[".venv", "venv", "env", ".env"] {
            let candidate = dir.join(venv_name).join("bin").join("manim");
            if candidate.is_file() {
                return Some(ResolvedManim {
                    bin_path: candidate,
                    venv_dir: Some(dir.join(venv_name)),
                });
            }
        }
        current = dir.parent();
    }

    // Check current working directory for virtualenvs
    if let Ok(cwd) = std::env::current_dir() {
        let mut cur = Some(cwd.as_path());
        while let Some(dir) = cur {
            for venv_name in &[".venv", "venv", "env", ".env"] {
                let candidate = dir.join(venv_name).join("bin").join("manim");
                if candidate.is_file() {
                    return Some(ResolvedManim {
                        bin_path: candidate,
                        venv_dir: Some(dir.join(venv_name)),
                    });
                }
            }
            cur = dir.parent();
        }
    }

    // Check active VIRTUAL_ENV environment variable
    if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
        let candidate = PathBuf::from(&venv).join("bin").join("manim");
        if candidate.is_file() {
            return Some(ResolvedManim {
                bin_path: candidate,
                venv_dir: Some(PathBuf::from(venv)),
            });
        }
    }

    // Check system PATH
    if let Ok(output) = Command::new("which").arg("manim").output() {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path_str.is_empty() {
                return Some(ResolvedManim {
                    bin_path: PathBuf::from(path_str),
                    venv_dir: None,
                });
            }
        }
    }

    None
}

pub fn create_fast_temp_dir() -> std::io::Result<tempfile::TempDir> {
    let shm = Path::new("/dev/shm");
    if shm.is_dir() && shm.exists() {
        tempfile::Builder::new().prefix("manim_live_").tempdir_in(shm)
    } else {
        tempfile::Builder::new().prefix("manim_live_").tempdir()
    }
}

impl BuildRunner {
    pub fn new(sender: Sender<BuildEvent>) -> Self {
        Self {
            sender,
            current_child: Arc::new(Mutex::new(None)),
            current_cancel: Arc::new(Mutex::new(None)),
        }
    }

    pub fn cancel(&self) {
        if let Ok(mut lock) = self.current_cancel.lock() {
            if let Some(token) = lock.take() {
                token.store(true, Ordering::SeqCst);
            }
        }
        if let Ok(mut lock) = self.current_child.lock() {
            if let Some(mut child) = lock.take() {
                let _ = child.kill();
            }
        }
    }

    pub fn trigger_build(
        &self,
        file_path: &Path,
        scene_name: Option<&str>,
        quality: Quality,
        fps: u32,
        explicit_manim: Option<&str>,
    ) {
        self.cancel();

        let fps = fps.max(1);
        let max_height = quality.max_height();

        let cancel_token = Arc::new(AtomicBool::new(false));
        if let Ok(mut lock) = self.current_cancel.lock() {
            *lock = Some(Arc::clone(&cancel_token));
        }

        let sender = self.sender.clone();
        let current_child = Arc::clone(&self.current_child);
        let is_cancelled = Arc::clone(&cancel_token);

        let file_path = file_path.to_path_buf();
        let scene_name = scene_name.map(|s| s.to_string());
        let explicit_manim = explicit_manim.map(|s| s.to_string());

        thread::Builder::new()
            .name("manim-runner".to_string())
            .spawn(move || {
                let start_time = Instant::now();

                let resolved = match resolve_manim(explicit_manim.as_deref(), &file_path) {
                    Some(r) => r,
                    None => {
                        let _ = sender.send(BuildEvent::OutputLine(
                            "✖ Could not locate 'manim' executable.".to_string(),
                        ));
                        let _ = sender.send(BuildEvent::OutputLine(
                            "  Checked system PATH and local virtual environments (.venv, venv).".to_string(),
                        ));
                        let _ = sender.send(BuildEvent::OutputLine(
                            "  Please activate your virtual environment or pass --manim <PATH>.".to_string(),
                        ));
                        let _ = sender.send(BuildEvent::Finished {
                            success: false,
                            duration: start_time.elapsed(),
                            output_path: None,
                            artifacts: FrameArtifacts {
                                frames: Vec::new(),
                                temp_dir_guard: None,
                            },
                        });
                        return;
                    }
                };

                let mut cmd = Command::new(&resolved.bin_path);

                if let Some(ref venv) = resolved.venv_dir {
                    cmd.env("VIRTUAL_ENV", venv);
                    if let Ok(cur_path) = std::env::var("PATH") {
                        let new_path = format!("{}:{}", venv.join("bin").display(), cur_path);
                        cmd.env("PATH", new_path);
                    }
                }

                if let Some(parent) = file_path.parent() {
                    if parent.exists() && !parent.as_os_str().is_empty() {
                        cmd.current_dir(parent);
                    }
                }

                cmd.arg(quality.to_flag());
                cmd.arg("--fps").arg(fps.to_string());
                cmd.arg(&file_path);

                if let Some(ref scene) = scene_name {
                    cmd.arg(scene);
                }

                cmd.stdout(Stdio::piped());
                cmd.stderr(Stdio::piped());

                let cmd_str = format!("{:?}", cmd);
                let _ = sender.send(BuildEvent::Started {
                    command: cmd_str.clone(),
                });

                let mut child = match cmd.spawn() {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = sender.send(BuildEvent::OutputLine(format!(
                            "Failed to start manim ({}): {}",
                            resolved.bin_path.display(),
                            e
                        )));
                        let _ = sender.send(BuildEvent::Finished {
                            success: false,
                            duration: start_time.elapsed(),
                            output_path: None,
                            artifacts: FrameArtifacts {
                                frames: Vec::new(),
                                temp_dir_guard: None,
                            },
                        });
                        return;
                    }
                };

                let stdout = child.stdout.take();
                let stderr = child.stderr.take();

                if let Ok(mut lock) = current_child.lock() {
                    *lock = Some(child);
                }

                let line_sender = sender.clone();
                let output_path_detected = Arc::new(Mutex::new(None));
                let path_det_stdout = Arc::clone(&output_path_detected);

                let progress_re = Regex::new(r"(\d{1,3})%").unwrap();
                let anim_cached_re = Regex::new(r"Animation\s+(\d+)\s*:\s*Using cached data").unwrap();
                let combining_re = Regex::new(r"Combining to Movie file").unwrap();
                let path_re = Regex::new(r#"(?:File ready at|Output file:?|Rendered .+ to|File written to|Saved to)\s*['"]?([^'"\r\n]+)['"]?"#).unwrap();
                let progress_re_stderr = progress_re.clone();
                let path_re_stderr = path_re.clone();
                let anim_cached_stderr = anim_cached_re.clone();
                let combining_stderr = combining_re.clone();

                let cancel_stdout = Arc::clone(&is_cancelled);
                // Spawn stdout reader
                let stdout_thread = thread::spawn(move || {
                    if let Some(out) = stdout {
                        let reader = BufReader::new(out);
                        for line in reader.lines().map_while(Result::ok) {
                            if cancel_stdout.load(Ordering::SeqCst) {
                                break;
                            }
                            // Check for progress
                            if let Some(caps) = progress_re.captures(&line) {
                                if let Ok(pct) = caps[1].parse::<f32>() {
                                    let _ = line_sender.send(BuildEvent::Progress {
                                        percentage: pct / 100.0,
                                        message: line.clone(),
                                    });
                                }
                            } else if let Some(caps) = anim_cached_re.captures(&line) {
                                if let Ok(num) = caps[1].parse::<f32>() {
                                    let pct = ((num + 1.0) * 0.16).min(0.80);
                                    let _ = line_sender.send(BuildEvent::Progress {
                                        percentage: pct,
                                        message: format!("Animation {}: cached", num as usize),
                                    });
                                }
                            } else if combining_re.is_match(&line) {
                                let _ = line_sender.send(BuildEvent::Progress {
                                    percentage: 0.85,
                                    message: "Combining movie file...".to_string(),
                                });
                            }
                            // Check for output file path
                            if let Some(caps) = path_re.captures(&line) {
                                let detected = caps[1].trim();
                                if !detected.is_empty() {
                                    if let Ok(mut lock) = path_det_stdout.lock() {
                                        *lock = Some(PathBuf::from(detected));
                                    }
                                }
                            }
                            let _ = line_sender.send(BuildEvent::OutputLine(line));
                        }
                    }
                });

                // Spawn stderr reader
                let err_sender = sender.clone();
                let path_det_stderr = Arc::clone(&output_path_detected);
                let cancel_stderr = Arc::clone(&is_cancelled);
                let stderr_thread = thread::spawn(move || {
                    if let Some(err) = stderr {
                        let reader = BufReader::new(err);
                        for line in reader.lines().map_while(Result::ok) {
                            if cancel_stderr.load(Ordering::SeqCst) {
                                break;
                            }
                            if let Some(caps) = progress_re_stderr.captures(&line) {
                                if let Ok(pct) = caps[1].parse::<f32>() {
                                    let _ = err_sender.send(BuildEvent::Progress {
                                        percentage: pct / 100.0,
                                        message: line.clone(),
                                    });
                                }
                            } else if let Some(caps) = anim_cached_stderr.captures(&line) {
                                if let Ok(num) = caps[1].parse::<f32>() {
                                    let pct = ((num + 1.0) * 0.16).min(0.80);
                                    let _ = err_sender.send(BuildEvent::Progress {
                                        percentage: pct,
                                        message: format!("Animation {}: cached", num as usize),
                                    });
                                }
                            } else if combining_stderr.is_match(&line) {
                                let _ = err_sender.send(BuildEvent::Progress {
                                    percentage: 0.85,
                                    message: "Combining movie file...".to_string(),
                                });
                            }
                            if let Some(caps) = path_re_stderr.captures(&line) {
                                let detected = caps[1].trim();
                                if !detected.is_empty() {
                                    if let Ok(mut lock) = path_det_stderr.lock() {
                                        *lock = Some(PathBuf::from(detected));
                                    }
                                }
                            }
                            let _ = err_sender.send(BuildEvent::OutputLine(line));
                        }
                    }
                });

                let _ = stdout_thread.join();
                let _ = stderr_thread.join();

                let exit_status = {
                    let mut lock = current_child.lock().unwrap();
                    if let Some(ref mut child) = *lock {
                        child.wait().ok()
                    } else {
                        None
                    }
                };

                if is_cancelled.load(Ordering::SeqCst) {
                    return;
                }

                let success = exit_status.map(|s| s.success()).unwrap_or(false);

                let target_ext = "mp4";

                let mut final_path = output_path_detected.lock().unwrap().take();
                if final_path.is_none() && success {
                    // Search media directory for the newest rendered file matching scene or stem
                    final_path = find_newest_media_file(&file_path, scene_name.as_deref(), target_ext);
                }

                let mut artifacts = FrameArtifacts {
                    frames: Vec::new(),
                    temp_dir_guard: None,
                };

                if success {
                    if let Some(ref path) = final_path {
                        if path.extension().is_some_and(|ext| ext == "mp4") {
                            let _ = sender.send(BuildEvent::Progress {
                                percentage: 0.90,
                                message: "Extracting animation frames via ffmpeg...".to_string(),
                            });
                            let _ = sender.send(BuildEvent::OutputLine("🎞 Extracting video frames for live playback...".to_string()));
                            artifacts = extract_video_frames(path, fps, max_height);
                            if artifacts.frames.is_empty() {
                                let _ = sender.send(BuildEvent::OutputLine("✖ Failed to extract video frames from rendered MP4.".to_string()));
                            } else {
                                let _ = sender.send(BuildEvent::OutputLine(format!("✔ Extracted {} video frames.", artifacts.frames.len())));
                                let _ = sender.send(BuildEvent::Progress {
                                    percentage: 1.0,
                                    message: "Frames ready".to_string(),
                                });
                            }
                        } else if path.exists() {
                            artifacts.frames.push(path.clone());
                        }
                    }
                }

                if is_cancelled.load(Ordering::SeqCst) {
                    return;
                }

                let final_success = success && !artifacts.frames.is_empty();
                let duration = start_time.elapsed();

                let _ = sender.send(BuildEvent::Finished {
                    success: final_success,
                    duration,
                    output_path: final_path,
                    artifacts,
                });
            })
            .expect("Failed to spawn manim runner thread");
    }
}

/// Recursively looks for the most recently modified file in the media folder matching target extension
fn find_newest_media_file(
    py_file: &Path,
    scene_name: Option<&str>,
    target_ext: &str,
) -> Option<PathBuf> {
    let mut dirs_to_visit = Vec::new();
    if let Some(parent) = py_file.parent() {
        let parent_media = parent.join("media");
        if parent_media.exists() {
            dirs_to_visit.push(parent_media);
        }
    }
    let cwd_media = PathBuf::from("media");
    if cwd_media.exists() && !dirs_to_visit.contains(&cwd_media) {
        dirs_to_visit.push(cwd_media);
    }
    if dirs_to_visit.is_empty() {
        return None;
    }

    let mut candidate: Option<(PathBuf, std::time::SystemTime)> = None;
    let stem = py_file.file_stem().and_then(|s| s.to_str()).unwrap_or("");

    while let Some(current_dir) = dirs_to_visit.pop() {
        if let Ok(entries) = fs::read_dir(current_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    if p.file_name().and_then(|s| s.to_str()) == Some("partial_movie_files") {
                        continue;
                    }
                    dirs_to_visit.push(p);
                } else if p.is_file() {
                    if let Some(ext) = p.extension() {
                        if ext == target_ext {
                            let filename = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                            let matches_scene = scene_name.is_none_or(|s| filename.contains(s));
                            let matches_stem = filename.contains(stem) || p.to_string_lossy().contains(stem);

                            if matches_scene || matches_stem {
                                if let Ok(meta) = p.metadata() {
                                    if let Ok(modified) = meta.modified() {
                                        match &candidate {
                                            None => candidate = Some((p.clone(), modified)),
                                            Some((_, prev_time)) => {
                                                if modified > *prev_time {
                                                    candidate = Some((p.clone(), modified));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    candidate.map(|(p, _)| p)
}

/// Extracts frames from an mp4 video using ffmpeg into a temporary directory backed by RAM disk (/dev/shm).
/// RAII TempDir ensures frames are cleaned up automatically when dropped.
fn extract_video_frames(video_path: &Path, fps: u32, max_height: u32) -> FrameArtifacts {
    let mut frames = Vec::new();
    let temp_dir = match create_fast_temp_dir() {
        Ok(td) => td,
        Err(_) => return FrameArtifacts { frames, temp_dir_guard: None },
    };

    let pattern = temp_dir.path().join("frame_%04d.png");

    let preview_fps = fps.clamp(1, 60);
    let max_h = max_height.clamp(240, 2160);
    let vf_filter = format!("fps={},scale=-2:'min(ih,{})'", preview_fps, max_h);

    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-v")
        .arg("error")
        .arg("-i")
        .arg(video_path)
        .arg("-vf")
        .arg(&vf_filter)
        .arg(&pattern)
        .status();

    if let Ok(s) = status {
        if s.success() {
            if let Ok(entries) = fs::read_dir(temp_dir.path()) {
                let mut paths: Vec<PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|ext| ext == "png"))
                    .collect();
                paths.sort();
                frames = paths;
            }
        }
    }

    FrameArtifacts {
        frames,
        temp_dir_guard: Some(Arc::new(temp_dir)),
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raii_temp_cleanup() {
        let temp_dir_path;
        {
            let temp_dir = create_fast_temp_dir().expect("Failed to create temp dir");
            temp_dir_path = temp_dir.path().to_path_buf();
            assert!(temp_dir_path.exists(), "Temp dir should exist while in scope");
            let artifacts = FrameArtifacts {
                frames: vec![temp_dir_path.join("frame_0001.png")],
                temp_dir_guard: Some(Arc::new(temp_dir)),
            };
            assert!(temp_dir_path.exists(), "Temp dir should exist while artifacts is in scope");
            drop(artifacts);
        }
        assert!(!temp_dir_path.exists(), "Temp dir must be removed after artifacts are dropped");
    }

    #[test]
    fn test_extract_frames() {
        let video = PathBuf::from("/home/chuck/Animaitons/manimaitons/media/videos/scene/480p15/PureManimSpirograph.mp4");
        if video.exists() {
            let artifacts = extract_video_frames(&video, 15, 720);
            assert!(!artifacts.frames.is_empty(), "Should extract frames from existing mp4");
            println!("Extracted {} frames!", artifacts.frames.len());
        }
    }

    #[test]
    fn test_full_build_pipeline() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let runner = BuildRunner::new(tx);
        let py = PathBuf::from("/home/chuck/Animaitons/manimaitons/scene.py");
        runner.trigger_build(
            &py,
            Some("PureManimSpirograph"),
            Quality::Low,
            15,
            None,
        );

        let timeout = Instant::now() + Duration::from_secs(10);
        let mut finished = false;
        let mut final_frames = Vec::new();
        while Instant::now() < timeout {
            if let Ok(event) = rx.recv_timeout(Duration::from_millis(100)) {
                match event {
                    BuildEvent::Started { command } => println!("Pipeline started: {}", command),
                    BuildEvent::OutputLine(l) => println!("Line: {}", l),
                    BuildEvent::Progress { percentage, message } => {
                        println!("Progress: {:.0}% - {}", percentage * 100.0, message)
                    }
                    BuildEvent::Finished {
                        success,
                        duration,
                        output_path,
                        artifacts,
                    } => {
                        println!(
                            "Pipeline finished: success={}, duration={:?}, path={:?}, frames={}",
                            success,
                            duration,
                            output_path,
                            artifacts.frames.len()
                        );
                        finished = true;
                        final_frames = artifacts.frames;
                        break;
                    }
                }
            }
        }

        assert!(finished, "Pipeline did not finish in 10s");
        assert!(!final_frames.is_empty(), "Pipeline should return frames");
    }
}
