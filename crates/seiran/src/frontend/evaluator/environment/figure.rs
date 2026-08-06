//! 図環境 — `figure`
//!
//! `\image` と `\caption` を [`HirNodeKind::Figure`] に変換する。

use crate::{
  frontend::{
    evaluator::{
      EvalError,
      environment::{body_scan, caption::extract_caption},
      opt_args::{OptType, OptValue, collect_command_opt_args, collect_environment_opt_args, find_string},
    },
    span_ext::ToSourceSpan,
    syntax::ast::{CommandView, EnvironmentView, extract_text_content},
  },
  length::Length,
  model::{CaptionPosition, HirBuilder, HirInline, HirNode, HirNodeKind},
};

/// `figure` 環境を評価する
///
/// # Errors
///
/// 未知の任意引数キー、`\image` の必須パラメータ不足などが発生した場合にエラーを返します。
pub(super) fn figure(view: &EnvironmentView, builder: &HirBuilder) -> Result<Vec<HirNode>, EvalError> {
  let opt_args = collect_environment_opt_args(view, &[("label", OptType::String)])?;
  let label = find_string(&opt_args, "label");

  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: "figure".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let id = builder.alloc(view.span());
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
    for cmd_view in
      body_scan::strict_command_calls(source, body, "figure", &["image", "caption"], "\\image と \\caption")?
    {
      match cmd_view.name() {
        "image" => {
          if image_path.is_some() {
            return Err(EvalError::DuplicateCommandInEnvironment {
              env: "figure".to_string(),
              name: "image".to_string(),
              span: cmd_view.span().to_source_span(),
            });
          }
          let extracted = extract_image(&cmd_view)?;
          image_path = Some(extracted.path);
          width = extracted.width;
          height = extracted.height;
          dpi = extracted.dpi;
          downsample = extracted.downsample;
        },
        "caption" => {
          if caption.is_some() {
            return Err(EvalError::DuplicateCommandInEnvironment {
              env: "figure".to_string(),
              name: "caption".to_string(),
              span: cmd_view.span().to_source_span(),
            });
          }
          if image_path.is_none() {
            caption_position = CaptionPosition::Top;
          }
          caption = Some(extract_caption(&cmd_view, builder)?);
        },
        _ => unreachable!("許可リスト外は strict_command_calls がエラーにする"),
      }
    }
  }

  let Some(image_path) = image_path else {
    return Err(EvalError::MissingEnvironmentArgument {
      name: "figure".to_string(),
      expected: "\\image コマンド".to_string(),
      span: view.span().to_source_span(),
    });
  };

  return Ok(vec![HirNode::new(
    id,
    HirNodeKind::Figure {
      image_path: crate::model::AssetId::new(image_path),
      width,
      height,
      dpi,
      downsample,
      caption,
      caption_position,
      label,
    },
  )]);
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
fn extract_image(view: &CommandView) -> Result<ImageArgs, EvalError> {
  let opt_args = collect_command_opt_args(
    view,
    &[
      ("width", OptType::Length),
      ("height", OptType::Length),
      ("dpi", OptType::Number),
      ("downsample", OptType::Bool),
    ],
  )?;

  let mut width: Option<Length> = None;
  let mut height: Option<Length> = None;
  let mut dpi: Option<u32> = None;
  let mut downsample: Option<bool> = None;
  for (key, value) in opt_args {
    match (key.as_str(), value) {
      ("width", OptValue::Length(l)) => width = Some(l),
      ("height", OptValue::Length(l)) => height = Some(l),
      ("dpi", OptValue::Number(n)) => {
        if !(n.is_finite() && n > 0.0 && n <= f64::from(u32::MAX)) {
          return Err(EvalError::InvalidOptArgValue {
            name: "image".to_string(),
            key: "dpi".to_string(),
            expected: "positive integer".to_string(),
            span: view.span().to_source_span(),
          });
        }
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let rounded = n.round() as u32;
        if rounded == 0 {
          return Err(EvalError::InvalidOptArgValue {
            name: "image".to_string(),
            key: "dpi".to_string(),
            expected: "positive integer".to_string(),
            span: view.span().to_source_span(),
          });
        }
        dpi = Some(rounded);
      },
      ("downsample", OptValue::Bool(b)) => downsample = Some(b),
      _ => unreachable!("collect_command_opt_args が未知キーと型不一致を弾くのでここには来ない"),
    }
  }

  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "image".to_string(),
      expected: "画像ファイルのパス".to_string(),
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "image".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let path = extract_text_content(view.source(), first_arg).trim().to_string();
  if path.is_empty() {
    return Err(EvalError::InvalidCommandArgument {
      name: "image".to_string(),
      reason: "画像ファイルのパスが空です".to_string(),
      span: view.span().to_source_span(),
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
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    frontend::evaluator::{evaluate_children_to_hir, lookup_env_parse_mode},
    model::HirInlineKind,
  };

  /// テスト用 `parse` ラッパ
  fn parse<'a>(
    source: &'a str,
    arena: &'a Bump,
  ) -> Result<&'a crate::frontend::syntax::green::GreenNode<'a>, crate::frontend::syntax::ParserError> {
    return crate::frontend::syntax::parse(source, arena, lookup_env_parse_mode);
  }

  #[test]
  fn figure_extracts_image_and_caption() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\image[width=80mm, height=60mm]{./images/seiran.jpg}\caption{タイトル}\end{figure}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let HirNodeKind::Figure {
      image_path,
      width,
      height,
      dpi,
      downsample,
      caption,
      caption_position,
      label,
      ..
    } = &result[0].kind
    else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(image_path.as_str(), "./images/seiran.jpg");
    assert!((width.expect("width 指定あり").to_mm() - 80.0).abs() < 1e-4);
    assert!((height.expect("height 指定あり").to_mm() - 60.0).abs() < 1e-4);
    assert!(dpi.is_none());
    assert!(downsample.is_none());
    let caption = caption.as_ref().expect("caption あり");
    assert_eq!(caption.len(), 1);
    assert!(matches!(&caption[0].kind, HirInlineKind::Text(t) if t == "タイトル"));
    assert_eq!(*caption_position, CaptionPosition::Bottom);
    assert!(label.is_none());
  }

  #[test]
  fn figure_caption_before_image_yields_top_position() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\caption{タイトル}\image[width=80mm, height=60mm]{a.png}\end{figure}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Figure {
      caption_position, ..
    } = &result[0].kind
    else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(*caption_position, CaptionPosition::Top);
  }

  #[test]
  fn figure_image_before_caption_yields_bottom_position() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\image[width=80mm, height=60mm]{a.png}\caption{タイトル}\end{figure}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Figure {
      caption_position, ..
    } = &result[0].kind
    else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(*caption_position, CaptionPosition::Bottom);
  }

  #[test]
  fn figure_captures_label() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}[label=fig:foo]\image[width=10mm, height=10mm]{a.png}\end{figure}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Figure { label, caption, .. } = &result[0].kind else {
      panic!("Figure が期待されます");
    };
    assert_eq!(label.as_deref(), Some("fig:foo"));
    assert!(caption.is_none());
  }

  #[test]
  fn figure_rejects_missing_image() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\caption{c}\end{figure}";
    let cst = parse(source, &arena).unwrap();

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
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let HirNodeKind::Figure {
      image_path,
      width,
      height,
      ..
    } = &result[0].kind
    else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(image_path.as_str(), "a.png");
    assert!(width.is_none());
    assert!(height.is_none());
  }

  #[test]
  fn figure_accepts_image_with_only_width() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\image[width=80mm]{a.png}\end{figure}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Figure { width, height, .. } = &result[0].kind else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert!((width.expect("width 指定あり").to_mm() - 80.0).abs() < 1e-4);
    assert!(height.is_none());
  }

  #[test]
  fn figure_rejects_unknown_opt_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}[foo=1]\image[width=1mm, height=1mm]{a}\end{figure}";
    let cst = parse(source, &arena).unwrap();

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
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Figure {
      dpi, downsample, ..
    } = &result[0].kind
    else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(*dpi, Some(600));
    assert_eq!(*downsample, Some(false));
  }

  #[test]
  fn image_rejects_zero_dpi() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\image[dpi=0]{a.png}\end{figure}";
    let cst = parse(source, &arena).unwrap();

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
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "dpi"));
  }
}
