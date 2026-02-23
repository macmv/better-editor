use std::{
  path::{Path, PathBuf},
  process::Command,
};

const VERSION: &str = "0.575.0";

pub fn import(path: &str) {
  let target_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

  download_icons(&target_dir);

  for icon in std::fs::read_dir(&target_dir.join("icons")).unwrap() {
    let icon = icon.unwrap();
    let path = icon.path();

    if path.extension() != Some("svg".as_ref()) {
      continue;
    }

    let _name = path.file_stem().unwrap().to_string_lossy().into_owned();
    let _svg = std::fs::read_to_string(&path).unwrap();
  }
}

fn download_icons(target_dir: &Path) {
  let target_path = std::path::Path::new(&target_dir).join("lucide.zip");

  let output = Command::new("curl").arg(format!(
    "https://github.com/lucide-icons/lucide/releases/download/{VERSION}/lucide-icons-{VERSION}.zip"
  )).arg("-L").arg("-o").arg(&target_path).output().unwrap();

  if !output.status.success() {
    panic!("Failed to download icons: {}", String::from_utf8_lossy(&output.stderr));
  }

  let output =
    Command::new("unzip").arg("-o").arg(target_path).arg("-d").arg(target_dir).output().unwrap();

  if !output.status.success() {
    panic!("Failed to unzip icons: {}", String::from_utf8_lossy(&output.stderr));
  }
}
