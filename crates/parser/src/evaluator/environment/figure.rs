//! 図環境 — `figure`
//!
//! `\begin{figure}...\end{figure}` を [`DocNode::Figure`] に変換します。
//! 本体には `\image` と `\caption` を 1 つずつ持つことを想定し、
//! それ以外のコマンド・テキストは無視します。
//!
//! ## 任意引数
//!
//! - `[label=fig:foo]` — `\ref` 解決用ラベル（任意）
//!
//! ## 本体内のコマンド
//!
//! - `\image[width=Xmm, height=Ymm]{path}` — 画像（必須）
//! - `\caption{...}` — キャプション（任意）

use syntax::ast::{CommandView, EnvironmentView, extract_text_content};

use crate::{
  document::{DocNode, InlineNode},
  evaluator::{
    EvalError, Evaluator,
    environment::body_scan,
    inline::extract_inline_nodes,
    opt_args::{OptType, OptValue, collect_command_opt_args, collect_environment_opt_args},
  },
};

/// `figure` 環境を評価する
///
/// `figure_count` をインクリメントし、本体内の `\image` / `\caption` を抽出して
/// [`DocNode::Figure`] を生成する。
///
/// # Errors
///
/// 未知の任意引数キー、`\image` の必須パラメータ不足などが発生した場合にエラーを返します。
pub(super) fn figure(view: &EnvironmentView, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
  let opt_args = collect_environment_opt_args(view, &[("label", OptType::String)])?;
  let label = opt_args.into_iter().find_map(|(key, value)| match (key.as_str(), value) {
    ("label", OptValue::String(s)) => Some(s),
    _ => None,
  });

  let number = evaluator.context.increment_figure().to_string();

  let source = view.source();
  let mut image_path: Option<String> = None;
  let mut width_mm: Option<f64> = None;
  let mut height_mm: Option<f64> = None;
  let mut caption: Option<Vec<InlineNode>> = None;

  if let Some(body) = view.body() {
    for cmd_view in body_scan::iter_command_calls(source, body) {
      match cmd_view.name() {
        "image" => {
          let (path, w, h) = extract_image(&cmd_view)?;
          image_path = Some(path);
          width_mm = Some(w);
          height_mm = Some(h);
        },
        "caption" => {
          caption = Some(extract_caption(&cmd_view)?);
        },
        _ => {},
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
  let Some(width_mm) = width_mm else {
    return Err(EvalError::MissingCommandArgument {
      name: "image".to_string(),
      expected: "width 任意引数（mm）".to_string(),
      span: view.span().into(),
    });
  };
  let Some(height_mm) = height_mm else {
    return Err(EvalError::MissingCommandArgument {
      name: "image".to_string(),
      expected: "height 任意引数（mm）".to_string(),
      span: view.span().into(),
    });
  };

  return Ok(vec![DocNode::Figure {
    image_path,
    width_mm,
    height_mm,
    caption,
    label,
    number,
  }]);
}

/// `\image[width=Xmm, height=Ymm]{path}` から path / width / height を抽出する
fn extract_image(view: &CommandView) -> Result<(String, f64, f64), EvalError> {
  let opt_args = collect_command_opt_args(view, &[("width", OptType::Length), ("height", OptType::Length)])?;

  let mut width_mm: Option<f64> = None;
  let mut height_mm: Option<f64> = None;
  for (key, value) in opt_args {
    match (key.as_str(), value) {
      ("width", OptValue::Length(mm)) => width_mm = Some(mm),
      ("height", OptValue::Length(mm)) => height_mm = Some(mm),
      _ => {},
    }
  }

  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "image".to_string(),
      expected: "画像ファイルのパス".to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "image".to_string(),
      span: view.span().into(),
    });
  }

  let path = extract_text_content(view.source(), first_arg).trim().to_string();

  let Some(width_mm) = width_mm else {
    return Err(EvalError::MissingCommandArgument {
      name: "image".to_string(),
      expected: "width 任意引数（mm）".to_string(),
      span: view.span().into(),
    });
  };
  let Some(height_mm) = height_mm else {
    return Err(EvalError::MissingCommandArgument {
      name: "image".to_string(),
      expected: "height 任意引数（mm）".to_string(),
      span: view.span().into(),
    });
  };

  return Ok((path, width_mm, height_mm));
}

/// `\caption{...}` の引数をインライン要素列に変換する
fn extract_caption(view: &CommandView) -> Result<Vec<InlineNode>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "caption".to_string(),
      expected: "キャプション本文".to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "caption".to_string(),
      span: view.span().into(),
    });
  }
  return extract_inline_nodes(view.source(), first_arg);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  /// テスト用 `parse` ラッパ
  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a syntax::green::GreenNode<'a>, syntax::ParserError> {
    return syntax::parse(source, arena, lookup_env_parse_mode);
  }

  #[test]
  fn figure_extracts_image_and_caption() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\image[width=80mm, height=60mm]{./images/seiran.jpg}\caption{タイトル}\end{figure}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let DocNode::Figure {
      image_path,
      width_mm,
      height_mm,
      caption,
      label,
      number,
    } = &result[0]
    else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(image_path, "./images/seiran.jpg");
    assert!((width_mm - 80.0).abs() < f64::EPSILON);
    assert!((height_mm - 60.0).abs() < f64::EPSILON);
    let caption = caption.as_ref().expect("caption あり");
    assert_eq!(caption.len(), 1);
    assert!(matches!(&caption[0], InlineNode::Text(t) if t == "タイトル"));
    assert!(label.is_none());
    assert_eq!(number, "1");
  }

  #[test]
  fn figure_captures_label() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}[label=fig:foo]\image[width=10mm, height=10mm]{a.png}\end{figure}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert
    let DocNode::Figure { label, caption, .. } = &result[0] else {
      panic!("Figure が期待されます");
    };
    assert_eq!(label.as_deref(), Some("fig:foo"));
    assert!(caption.is_none());
  }

  #[test]
  fn figure_assigns_sequential_numbers() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}\image[width=1mm, height=1mm]{a}\end{figure}\begin{figure}\image[width=1mm, height=1mm]{b}\end{figure}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 2);
    let numbers: Vec<&str> = result
      .iter()
      .map(|n| match n {
        DocNode::Figure { number, .. } => number.as_str(),
        _ => panic!("Figure が期待されます: {n:?}"),
      })
      .collect();
    assert_eq!(numbers, vec!["1", "2"]);
  }

  #[test]
  fn figure_rejects_missing_image() {
    // Arrange — image なしはエラー
    let arena = Bump::new();
    let source = r"\begin{figure}\caption{c}\end{figure}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::MissingEnvironmentArgument { ref name, .. }) if name == "figure"));
  }

  #[test]
  fn figure_rejects_image_missing_size() {
    // Arrange — width / height は必須
    let arena = Bump::new();
    let source = r"\begin{figure}\image{a.png}\end{figure}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::MissingCommandArgument { ref name, .. }) if name == "image"));
  }

  #[test]
  fn figure_rejects_unknown_opt_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{figure}[foo=1]\image[width=1mm, height=1mm]{a}\end{figure}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }
}
