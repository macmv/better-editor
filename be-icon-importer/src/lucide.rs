use std::{path::Path, process::Command};

const VERSION: &str = "0.575.0";

pub fn download_icons(target_dir: &Path) {
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
