use std::{
  path::{Path, PathBuf},
  process::Command,
};

pub fn download(target_dir: &Path, name: &str) -> PathBuf {
  std::fs::create_dir_all(target_dir.join("devicons")).unwrap();
  let target_path = target_dir.join("devicons").join(format!("{name}.svg"));

  let output = Command::new("curl")
    .arg(format!(
      "https://cdn.jsdelivr.net/gh/devicons/devicon@latest/icons/{name}/{name}-original.svg"
    ))
    .arg("-L")
    .arg("-o")
    .arg(&target_path)
    .output()
    .unwrap();

  if !output.status.success() {
    panic!("Failed to download icons: {}", String::from_utf8_lossy(&output.stderr));
  }

  target_path
}
