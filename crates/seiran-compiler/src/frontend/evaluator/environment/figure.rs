//! 図環境 — `figure`
//!
//! `\image` と `\caption` を [`HirNodeKind::Figure`] に変換する。

use crate::{
  document::{CaptionPosition, HirFigure, HirInline, HirNode, HirNodeKind},
  frontend::{
    evaluator::{
      EvalContext, EvalError, arity,
      environment::{body_scan, caption::extract_caption},
      opt_args::{self, OptKey, collect_command_opt_args, collect_environment_opt_args},
    },
    syntax::view::{CommandView, EnvironmentView, extract_text_content},
  },
  length::Length,
};

/// `figure` 環境の `[label=...]`（`\ref` からの参照用）
const LABEL: OptKey<String> = opt_args::string("label");
/// `\image[width=...]`（描画幅。0 と負値は収集時に拒否される）
const WIDTH: OptKey<Length> = opt_args::positive_length("width");
/// `\image[height=...]`（描画高さ。同上）
const HEIGHT: OptKey<Length> = opt_args::positive_length("height");
/// `\image[dpi=N]`（per-image DPI 上限。小数は四捨五入される — #689）
const DPI: OptKey<u32> = opt_args::rounded_int("dpi");
/// `\image[downsample=...]`（per-image ダウンサンプリング）
const DOWNSAMPLE: OptKey<bool> = opt_args::boolean("downsample");

/// `figure` 環境の本体に書けるコマンド
#[derive(Debug, Clone, Copy)]
enum FigureCommand {
  /// `\image[...]{path}` — 図の実体
  Image,
  /// `\caption{...}` — 図のキャプション
  Caption,
}

/// `figure` 環境の本体で許可するコマンドと種別
const FIGURE_COMMANDS: &[(&str, FigureCommand)] = &[
  ("image", FigureCommand::Image),
  ("caption", FigureCommand::Caption),
];

/// `figure` 環境を評価する
///
/// # Errors
///
/// 未知の任意引数キー、`\image` の必須パラメータ不足などが発生した場合にエラーを返します。
pub(super) fn figure(view: &EnvironmentView<'_>, ctx: &EvalContext<'_>) -> Result<HirNode, EvalError> {
  let opts = collect_environment_opt_args(view, &[LABEL.decl()])?;
  let label = opts.get(LABEL);

  arity::no_environment_args(view)?;

  let id = ctx.alloc(view.span());
  let source = view.source();
  let mut image_path: Option<String> = None;
  let mut width: Option<Length> = None;
  let mut height: Option<Length> = None;
  let mut dpi: Option<u32> = None;
  let mut downsample: Option<bool> = None;
  let mut caption: Option<Vec<HirInline>> = None;
  // `\caption` が `\image` よりソース上で先に現れた場合のみ Top、それ以外は Bottom（既定）
  let mut caption_position = CaptionPosition::Bottom;

  if let Some(body) = view.body() {
    for (command, cmd_view) in
      body_scan::strict_command_calls(source, body.children, "figure", FIGURE_COMMANDS, "\\image と \\caption")?
    {
      match command {
        FigureCommand::Image => {
          if image_path.is_some() {
            return Err(EvalError::DuplicateCommandInEnvironment {
              env: "figure".to_string(),
              name: "image".to_string(),
              span: cmd_view.span().into(),
            });
          }
          let extracted = extract_image(&cmd_view)?;
          image_path = Some(extracted.path);
          width = extracted.width;
          height = extracted.height;
          dpi = extracted.dpi;
          downsample = extracted.downsample;
        },
        FigureCommand::Caption => {
          if caption.is_some() {
            return Err(EvalError::DuplicateCommandInEnvironment {
              env: "figure".to_string(),
              name: "caption".to_string(),
              span: cmd_view.span().into(),
            });
          }
          if image_path.is_none() {
            caption_position = CaptionPosition::Top;
          }
          caption = Some(extract_caption(&cmd_view, ctx)?);
        },
      }
    }
  }

  let Some(image_path) = image_path else {
    return Err(EvalError::MissingEnvironmentArgument {
      name: "figure".to_string(),
      expected: "\\image コマンド".to_string(),
      span: view.span().into(),
    });
  };

  return Ok(HirNode::new(
    id,
    HirNodeKind::Figure(HirFigure {
      image_path: ctx.resolve_path(&image_path),
      width,
      height,
      dpi,
      downsample,
      caption,
      caption_position,
      label,
    }),
  ));
}

/// `\image` から抽出される情報の集約構造体
struct ImageArgs {
  /// 画像ファイルへのパス
  path: String,
  /// 描画幅
  width: Option<Length>,
  /// 描画高さ
  height: Option<Length>,
  /// per-image DPI 上限
  dpi: Option<u32>,
  /// per-image ダウンサンプリング ON/OFF
  downsample: Option<bool>,
}

/// `\image[width=Xmm, height=Ymm, dpi=N, downsample=true|false]{path}` から各引数を抽出する
fn extract_image(view: &CommandView<'_>) -> Result<ImageArgs, EvalError> {
  let opts = collect_command_opt_args(view, &[WIDTH.decl(), HEIGHT.decl(), DPI.decl(), DOWNSAMPLE.decl()])?;
  let width = opts.get(WIDTH);
  let height = opts.get(HEIGHT);
  let dpi = opts.get(DPI);
  let downsample = opts.get(DOWNSAMPLE);

  let first_arg = arity::exactly_one_arg(view, "画像ファイルのパス")?;

  let path = extract_text_content(view.source(), first_arg).trim().to_string();
  if path.is_empty() {
    return Err(EvalError::InvalidCommandArgument {
      name: "image".to_string(),
      reason: "画像ファイルのパスが空です".to_string(),
      span: view.span().into(),
    });
  }

  return Ok(ImageArgs {
    path,
    width,
    height,
    dpi,
    downsample,
  });
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::HirInlineKind,
    frontend::evaluator::{evaluate_children_to_hir, test_support},
  };

  #[test]
  fn figure_extracts_image_and_caption() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\image[width=80mm, height=60mm]{./images/seiran.jpg}\caption{タイトル}\end{figure}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let HirNodeKind::Figure(figure) = &result[0].kind else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(figure.image_path.to_string(), "./images/seiran.jpg");
    assert!((figure.width.expect("width 指定あり").to_mm() - 80.0).abs() < 1e-4);
    assert!((figure.height.expect("height 指定あり").to_mm() - 60.0).abs() < 1e-4);
    assert!(figure.dpi.is_none());
    assert!(figure.downsample.is_none());
    let caption = figure.caption.as_ref().expect("caption あり");
    assert_eq!(caption.len(), 1);
    assert!(matches!(&caption[0].kind, HirInlineKind::Text(t) if t == "タイトル"));
    assert_eq!(figure.caption_position, CaptionPosition::Bottom);
    assert!(figure.label.is_none());
  }

  #[test]
  fn figure_caption_before_image_yields_top_position() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\caption{タイトル}\image[width=80mm, height=60mm]{a.png}\end{figure}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Figure(figure) = &result[0].kind else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(figure.caption_position, CaptionPosition::Top);
  }

  #[test]
  fn figure_image_before_caption_yields_bottom_position() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\image[width=80mm, height=60mm]{a.png}\caption{タイトル}\end{figure}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Figure(figure) = &result[0].kind else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(figure.caption_position, CaptionPosition::Bottom);
  }

  #[test]
  fn figure_captures_label() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}[label=fig:foo]\image[width=10mm, height=10mm]{a.png}\end{figure}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Figure(figure) = &result[0].kind else {
      panic!("Figure が期待されます");
    };
    assert_eq!(figure.label.as_deref(), Some("fig:foo"));
    assert!(figure.caption.is_none());
  }

  #[test]
  fn figure_rejects_missing_image() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\caption{c}\end{figure}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::MissingEnvironmentArgument { ref name, .. }) if name == "figure"));
  }

  #[test]
  fn figure_accepts_image_without_size() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\image{a.png}\end{figure}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let HirNodeKind::Figure(figure) = &result[0].kind else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(figure.image_path.to_string(), "a.png");
    assert!(figure.width.is_none());
    assert!(figure.height.is_none());
  }

  #[test]
  fn figure_accepts_image_with_only_width() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\image[width=80mm]{a.png}\end{figure}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Figure(figure) = &result[0].kind else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert!((figure.width.expect("width 指定あり").to_mm() - 80.0).abs() < 1e-4);
    assert!(figure.height.is_none());
  }

  #[test]
  fn figure_rejects_unknown_opt_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}[foo=1]\image[width=1mm, height=1mm]{a}\end{figure}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }

  #[test]
  fn image_captures_dpi_and_downsample() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\image[width=80mm, dpi=600, downsample=false]{a.png}\end{figure}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Figure(figure) = &result[0].kind else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(figure.dpi, Some(600));
    assert_eq!(figure.downsample, Some(false));
  }

  #[test]
  fn image_rejects_zero_dpi() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\image[dpi=0]{a.png}\end{figure}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "dpi"));
  }

  #[test]
  fn image_rejects_negative_dpi() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\image[dpi=-150]{a.png}\end{figure}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "dpi"));
  }

  #[test]
  fn image_rejects_zero_width() {
    // Arrange — 描画寸法 0 は krilla が受け付けないので、描画段まで運ばずここで弾く（#378）
    let arena = Bump::new();
    let source = r"\begin{figure}\image[width=0mm]{a.png}\end{figure}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "width"));
  }

  #[test]
  fn image_rejects_negative_height() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\image[height=-5mm]{a.png}\end{figure}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "height"));
  }
}
