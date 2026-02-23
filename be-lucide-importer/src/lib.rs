use std::{
  path::{Path, PathBuf},
  process::Command,
};
use usvg::{Node, Tree, tiny_skia_path::Point};

const VERSION: &str = "0.575.0";

pub fn import(path: &str) {
  println!("cargo::rerun-if-changed=build.rs");

  let target_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

  download_icons(&target_dir);

  let mut icons = vec![];

  for icon in std::fs::read_dir(&target_dir.join("icons")).unwrap() {
    let icon = icon.unwrap();
    let path = icon.path();

    if path.extension() != Some("svg".as_ref()) {
      continue;
    }

    let name = path.file_stem().unwrap().to_string_lossy().into_owned();
    let svg = std::fs::read_to_string(&path).unwrap();
    let source = import_svg(&svg);
    icons.push((name, source));
  }

  icons.sort_by_key(|(name, _)| name.clone());

  let mut content = String::new();

  content.push_str("use std::sync::LazyLock;\n");
  content.push_str("use kurbo::{BezPath, PathEl, Point};\n");

  for (name, source) in icons {
    content.push_str(&format!(
      "pub const {}: LazyLock<BezPath> = LazyLock::new(|| BezPath::from_vec(vec![{}]));\n",
      to_upper_snake(&name),
      source
    ));
  }

  std::fs::create_dir_all(Path::new(&path).parent().unwrap()).unwrap();
  std::fs::write(path, content).unwrap();

  Command::new("rustfmt").arg(path).status().unwrap();
}

fn import_svg(content: &str) -> String {
  let tree = Tree::from_str(content, &usvg::Options::default()).unwrap();
  let paths = collect_paths(tree.root());

  let mut content = String::new();

  for path in paths {
    content.push_str(&path_to_source(&path));
    content.push_str("\n");
  }

  content
}

fn collect_paths(group: &usvg::Group) -> Vec<usvg::tiny_skia_path::Path> {
  let mut paths = Vec::new();
  collect_group_paths(group, &mut paths);
  paths
}

fn collect_group_paths(group: &usvg::Group, paths: &mut Vec<usvg::tiny_skia_path::Path>) {
  for node in group.children() {
    match node {
      Node::Group(group) => collect_group_paths(group, paths),
      Node::Path(path) => {
        let transformed = path.data().clone().transform(path.abs_transform());
        paths.push(transformed.unwrap_or_else(|| path.data().clone()));
      }
      Node::Image(_) | Node::Text(_) => {}
    }
  }
}

fn path_to_source(path: &usvg::tiny_skia_path::Path) -> String {
  use usvg::tiny_skia_path::PathSegment;

  let mut out = String::new();

  for segment in path.segments() {
    match segment {
      PathSegment::MoveTo(p) => {
        out.push_str("PathEl::MoveTo(");
        write_point(&mut out, p);
        out.push_str(")");
      }
      PathSegment::LineTo(p) => {
        out.push_str("PathEl::LineTo(");
        write_point(&mut out, p);
        out.push_str(")");
      }
      PathSegment::QuadTo(ctrl, end) => {
        out.push_str("PathEl::QuadTo(");
        write_point(&mut out, ctrl);
        out.push_str(", ");
        write_point(&mut out, end);
        out.push_str(")");
      }
      PathSegment::CubicTo(c1, c2, end) => {
        out.push_str("PathEl::CurveTo(");
        write_point(&mut out, c1);
        out.push_str(", ");
        write_point(&mut out, c2);
        out.push_str(", ");
        write_point(&mut out, end);
        out.push_str(")");
      }
      PathSegment::Close => out.push_str("PathEl::ClosePath"),
    }

    out.push_str(", ");
  }

  out
}

fn write_point(out: &mut String, p: Point) {
  out.push_str("Point { x: ");
  out.push_str(&format!("{:.6}", p.x));
  out.push_str(", y: ");
  out.push_str(&format!("{:.6}", p.y));
  out.push_str(" }");
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

fn to_upper_snake(s: &str) -> String { s.to_ascii_uppercase().replace('-', "_") }
