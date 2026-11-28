//! 図環境 — `figure` と、その本体に並べる副図環境 `subfigure`
//!
//! `figure` の本体は `\image` 1 個か `subfigure` 1 個以上のどちらかと任意の `\caption` 1 個、`subfigure` の本体は
//! `\image` 1 個と任意の `\caption` 1 個で、[`HirNodeKind::Figure`] に変換する。キャプションの上下はソース上の出現順で
//! 決まる（本体より前なら上、後ろなら下）。

use std::num::NonZeroU32;

use miette::SourceSpan;

use crate::{
  document::{
    CaptionPosition, FigureBody, HirFigure, HirImage, HirInline, HirNode, HirNodeKind, HirSubfigure, ImageFormat,
  },
  frontend::{
    evaluator::{
      EvalContext, EvalError,
      environment::{
        body_scan::{self, BodyItem},
        caption,
      },
      opt_args::{self, OptArgs, OptDecl, OptKey},
      signature::{Bound, Signature},
      string_arg,
    },
    syntax::{
      ArgMode, CstNode,
      view::{CommandView, EnvironmentView},
    },
  },
  length::Length,
};

/// `figure` / `subfigure` 環境の `[label=...]`（`\ref` からの参照用）
const LABEL: OptKey<String> = opt_args::string("label");
/// `figure[per_row=N]`（1 行に並べる副図の個数。小数と 1 未満は収集時に拒否される）
const PER_ROW: OptKey<u32> = opt_args::positive_int("per_row");
/// `\image[width=...]`（描画幅。0 と負値は収集時に拒否される）
const WIDTH: OptKey<Length> = opt_args::positive_length("width");
/// `\image[height=...]`（描画高さ。同上）
const HEIGHT: OptKey<Length> = opt_args::positive_length("height");
/// `\image[dpi=N]`（per-image DPI 上限。小数と 1 未満は収集時に拒否される）
const DPI: OptKey<u32> = opt_args::positive_int("dpi");
/// `\image[downsample=...]`（per-image ダウンサンプリング）
const DOWNSAMPLE: OptKey<bool> = opt_args::boolean("downsample");
/// `\image[page=N]`（PDF のページ。小数と 1 未満は収集時に拒否される）
const PAGE: OptKey<u32> = opt_args::positive_int("page");
/// `figure` 環境が受け付けるキー
const FIGURE_KEYS: &[OptDecl] = &[LABEL.decl(), PER_ROW.decl()];
/// `subfigure` 環境が受け付けるキー
const SUBFIGURE_KEYS: &[OptDecl] = &[LABEL.decl()];
/// `\image` が受け付けるキー
const IMAGE_KEYS: &[OptDecl] = &[
  WIDTH.decl(),
  HEIGHT.decl(),
  DPI.decl(),
  DOWNSAMPLE.decl(),
  PAGE.decl(),
];

/// `figure` 環境の署名
pub(super) const SIGNATURE: Signature<0> = Signature::keys_only(FIGURE_KEYS);
/// `subfigure` 環境の署名
pub(super) const SUBFIGURE_SIGNATURE: Signature<0> = Signature::keys_only(SUBFIGURE_KEYS);
/// `\image` の署名
const IMAGE: Signature<1> = Signature::new([ArgMode::Inherit], "画像ファイルのパス", IMAGE_KEYS);

/// `figure` / `subfigure` 環境の本体に書けるコマンド
#[derive(Debug, Clone, Copy)]
enum FigureCommand {
  /// `\image[...]{path}` — 図の実体
  Image,
  /// `\caption{...}` — 図のキャプション
  Caption,
}

/// `figure` / `subfigure` 環境の本体で許可するコマンドと種別と署名
const FIGURE_COMMANDS: &[(&str, FigureCommand, Signature<1>)] = &[
  ("image", FigureCommand::Image, IMAGE),
  ("caption", FigureCommand::Caption, caption::SIGNATURE),
];

/// `figure` 環境の本体で許可する入れ子の環境と署名
const FIGURE_ENVIRONMENTS: &[(&str, Signature<0>)] = &[("subfigure", SUBFIGURE_SIGNATURE)];

/// `figure` 環境を評価する
///
/// # Errors
///
/// 本体の許可外コンテンツ、`\image` と `subfigure` の欠落・混在、キャプションの位置の矛盾、副図の無い図への
/// `per_row` などが発生した場合にエラーを返します。
pub(super) fn figure(view: &EnvironmentView<'_>, ctx: &EvalContext<'_>, opts: &OptArgs) -> Result<HirNode, EvalError> {
  let label = opts.get(LABEL);
  let per_row = opts.get(PER_ROW);

  let id = ctx.alloc(view.span());
  let source = view.source();
  let mut image: Option<HirImage> = None;
  let mut subfigures: Vec<HirSubfigure> = Vec::new();
  // 最初の副キャプションの位置（以降の副キャプションはこれと揃わなければならない）
  let mut subcaption_position: Option<CaptionPosition> = None;
  let mut caption: Option<(Vec<HirInline>, SourceSpan)> = None;
  // `\caption` が本体（`\image` か最初の `subfigure`）よりソース上で先に現れた場合のみ Top、それ以外は Bottom（既定）
  let mut caption_position = CaptionPosition::Bottom;

  if let Some(body) = view.body() {
    for item in body_scan::collect_allowed_items(
      source,
      body.children,
      "figure",
      FIGURE_COMMANDS,
      FIGURE_ENVIRONMENTS,
      "\\image・\\caption・subfigure",
    )? {
      match item {
        BodyItem::Command(FigureCommand::Image, cmd_view, Bound { args: [arg], opts }) => {
          if image.is_some() {
            return Err(duplicate_command("figure", "image", &cmd_view));
          }
          if !subfigures.is_empty() {
            return Err(EvalError::FigureImageWithSubfigures {
              span: cmd_view.span().into(),
            });
          }
          image = Some(extract_image(&cmd_view, arg, &opts, ctx)?);
        },
        BodyItem::Command(FigureCommand::Caption, cmd_view, Bound { args: [arg], .. }) => {
          if caption.is_some() {
            return Err(duplicate_command("figure", "caption", &cmd_view));
          }
          if image.is_none() && subfigures.is_empty() {
            caption_position = CaptionPosition::Top;
          }
          caption = Some((caption::evaluate_caption(&cmd_view, arg, ctx)?, cmd_view.span().into()));
        },
        BodyItem::Environment(sub_view, sub_opts) => {
          if image.is_some() {
            return Err(EvalError::FigureImageWithSubfigures {
              span: sub_view.span().into(),
            });
          }
          // 本体の後ろ（Bottom）に置いた `\caption` の後にまだ副図が続く
          if let Some((_, caption_span)) = &caption
            && caption_position == CaptionPosition::Bottom
          {
            return Err(EvalError::FigureCaptionBetweenSubfigures {
              span: *caption_span,
            });
          }
          let (subfigure, position) = subfigure(&sub_view, ctx, &sub_opts)?;
          if let Some((position, caption_span)) = position {
            match subcaption_position {
              None => subcaption_position = Some(position),
              Some(first) if first != position => {
                return Err(EvalError::SubcaptionPositionMixed { span: caption_span });
              },
              Some(_) => {},
            }
          }
          subfigures.push(subfigure);
        },
      }
    }
  }

  let body = match (image, subfigures.is_empty()) {
    (Some(image), true) => {
      if per_row.is_some() {
        return Err(EvalError::PerRowWithoutSubfigures {
          span: view.span().into(),
        });
      }
      FigureBody::Image(image)
    },
    (None, false) => {
      let Ok(count) = u32::try_from(subfigures.len()) else {
        unreachable!("副図の個数はソースの長さ（u32 の span で表される）を超えない");
      };
      FigureBody::Subfigures {
        per_row: NonZeroU32::new(per_row.unwrap_or(count))
          .expect("per_row は positive_int が 1 以上だけを通し、省略時の副図の個数も 1 以上"),
        caption_position: subcaption_position.unwrap_or(CaptionPosition::Bottom),
        items: subfigures,
      }
    },
    (None, true) => {
      return Err(EvalError::MissingEnvironmentArgument {
        name: "figure".to_string(),
        expected: "\\image コマンドか subfigure 環境".to_string(),
        span: view.span().into(),
      });
    },
    (Some(_), false) => unreachable!("\\image と subfigure の混在は本体の走査で拒否済み"),
  };

  return Ok(HirNode::new(
    id,
    HirNodeKind::Figure(HirFigure {
      body,
      caption: caption.map(|(inlines, _)| return inlines),
      caption_position,
      label,
    }),
  ));
}

/// `figure` の本体の直下の `subfigure` 環境を評価し、副図と、副キャプションがあればその位置と `\caption` の位置を返す
///
/// # Errors
///
/// 本体の許可外コンテンツ（入れ子の `subfigure` を含む）、`\image` の欠落、`\image` / `\caption` の重複
fn subfigure(
  view: &EnvironmentView<'_>,
  ctx: &EvalContext<'_>,
  opts: &OptArgs,
) -> Result<(HirSubfigure, Option<(CaptionPosition, SourceSpan)>), EvalError> {
  let id = ctx.alloc(view.span());
  let mut image: Option<HirImage> = None;
  let mut caption: Option<(Vec<HirInline>, CaptionPosition, SourceSpan)> = None;

  if let Some(body) = view.body() {
    for (command, cmd_view, Bound { args: [arg], opts }) in body_scan::collect_allowed_commands(
      view.source(),
      body.children,
      "subfigure",
      FIGURE_COMMANDS,
      "\\image と \\caption",
    )? {
      match command {
        FigureCommand::Image => {
          if image.is_some() {
            return Err(duplicate_command("subfigure", "image", &cmd_view));
          }
          image = Some(extract_image(&cmd_view, arg, &opts, ctx)?);
        },
        FigureCommand::Caption => {
          if caption.is_some() {
            return Err(duplicate_command("subfigure", "caption", &cmd_view));
          }
          let position = if image.is_none() {
            CaptionPosition::Top
          } else {
            CaptionPosition::Bottom
          };
          caption = Some((caption::evaluate_caption(&cmd_view, arg, ctx)?, position, cmd_view.span().into()));
        },
      }
    }
  }

  let Some(image) = image else {
    return Err(EvalError::MissingEnvironmentArgument {
      name: "subfigure".to_string(),
      expected: "\\image コマンド".to_string(),
      span: view.span().into(),
    });
  };
  let (inlines, position) = match caption {
    Some((inlines, position, span)) => (Some(inlines), Some((position, span))),
    None => (None, None),
  };
  return Ok((
    HirSubfigure {
      id,
      image,
      caption: inlines,
      label: opts.get(LABEL),
    },
    position,
  ));
}

/// 環境 `env` の本体で 1 回しか書けないコマンド `name` の 2 回目 `view` の診断
fn duplicate_command(env: &str, name: &str, view: &CommandView<'_>) -> EvalError {
  return EvalError::DuplicateCommandInEnvironment {
    env: env.to_string(),
    name: name.to_string(),
    span: view.span().into(),
  };
}

/// `\image[width=Xmm, height=Ymm, dpi=N, downsample=true|false, page=N]{path}` から各引数を抽出する
fn extract_image(
  view: &CommandView<'_>,
  path_arg: &CstNode<'_>,
  opts: &OptArgs,
  ctx: &EvalContext<'_>,
) -> Result<HirImage, EvalError> {
  let width = opts.get(WIDTH);
  let height = opts.get(HEIGHT);
  let dpi = opts.get(DPI);
  let downsample = opts.get(DOWNSAMPLE);
  let page = opts.get(PAGE);

  let path = string_arg::text(view, path_arg)?.trim().to_string();
  if path.is_empty() {
    return Err(EvalError::InvalidCommandArgument {
      name: "image".to_string(),
      reason: "画像ファイルのパスが空です".to_string(),
      span: view.span().into(),
    });
  }

  let Some(format) = ImageFormat::from_path(&path) else {
    return Err(EvalError::UnsupportedImageFormat {
      path,
      span: view.span().into(),
    });
  };
  // 効かないことが拡張子（字面）だけで決まるオプションは、ファイルを読む前にここで拒否する
  let (raster, pdf) = match format {
    ImageFormat::Png | ImageFormat::Jpeg => (true, false),
    ImageFormat::Svg => (false, false),
    ImageFormat::Pdf => (false, true),
  };
  let format_specific = [
    ("page", page.is_some(), pdf, "PDF"),
    ("dpi", dpi.is_some(), raster, "PNG / JPEG"),
    ("downsample", downsample.is_some(), raster, "PNG / JPEG"),
  ];
  for (key, written, applies, applies_to) in format_specific {
    if written && !applies {
      return Err(EvalError::ImageOptionNotApplicable {
        key: key.to_string(),
        format,
        applies_to: applies_to.to_string(),
        span: view.span().into(),
      });
    }
  }

  return Ok(HirImage {
    site: ctx.alloc(view.span()),
    path: ctx.resolve_path(&path),
    format,
    width,
    height,
    dpi,
    downsample,
    page,
  });
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::HirInlineKind,
    frontend::{
      self,
      evaluator::{evaluate_children_to_hir, test_support},
    },
    source::{SourceId, Span},
  };

  /// 単独の画像を 1 枚持つ図の画像
  fn only_image(figure: &HirFigure) -> &HirImage {
    return figure.images().next().expect("画像が 1 枚あるはず");
  }

  #[test]
  fn image_site_spans_the_image_command() {
    let source = r"\begin{figure}\image[width=80mm]{a.png}\end{figure}";

    let hir = frontend::test_support::parse_for_test(source, SourceId::new(0)).unwrap();

    let HirNodeKind::Figure(figure) = &hir.nodes[0].kind else {
      panic!("Figure が期待されます: {:?}", hir.nodes[0]);
    };
    let command = r"\image[width=80mm]{a.png}";
    let start = u32::try_from(source.find(command).unwrap()).unwrap();
    let end = start + u32::try_from(command.len()).unwrap();
    assert_eq!(hir.spans.span_of(only_image(figure).site), Span::new(start, end));
  }

  #[test]
  fn figure_extracts_image_and_caption() {
    let arena = Bump::new();
    let source = r"\begin{figure}\image[width=80mm, height=60mm]{./images/seiran.jpg}\caption{タイトル}\end{figure}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    assert_eq!(result.len(), 1);
    let HirNodeKind::Figure(figure) = &result[0].kind else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(only_image(figure).path.to_string(), "./images/seiran.jpg");
    assert!((only_image(figure).width.expect("width 指定あり").to_mm() - 80.0).abs() < 1e-4);
    assert!((only_image(figure).height.expect("height 指定あり").to_mm() - 60.0).abs() < 1e-4);
    assert!(only_image(figure).dpi.is_none());
    assert!(only_image(figure).downsample.is_none());
    let caption = figure.caption.as_ref().expect("caption あり");
    assert_eq!(caption.len(), 1);
    assert!(matches!(&caption[0].kind, HirInlineKind::Text(t) if t == "タイトル"));
    assert_eq!(figure.caption_position, CaptionPosition::Bottom);
    assert!(figure.label.is_none());
  }

  #[test]
  fn figure_caption_before_image_yields_top_position() {
    let arena = Bump::new();
    let source = r"\begin{figure}\caption{タイトル}\image[width=80mm, height=60mm]{a.png}\end{figure}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::Figure(figure) = &result[0].kind else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(figure.caption_position, CaptionPosition::Top);
  }

  #[test]
  fn figure_captures_label() {
    let arena = Bump::new();
    let source = r"\begin{figure}[label=fig:foo]\image[width=10mm, height=10mm]{a.png}\end{figure}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::Figure(figure) = &result[0].kind else {
      panic!("Figure が期待されます");
    };
    assert_eq!(figure.label.as_deref(), Some("fig:foo"));
    assert!(figure.caption.is_none());
  }

  #[test]
  fn figure_rejects_missing_image() {
    let arena = Bump::new();
    let source = r"\begin{figure}\caption{c}\end{figure}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::MissingEnvironmentArgument { ref name, .. }) if name == "figure"));
  }

  #[test]
  fn figure_accepts_image_without_size() {
    let arena = Bump::new();
    let source = r"\begin{figure}\image{a.png}\end{figure}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    assert_eq!(result.len(), 1);
    let HirNodeKind::Figure(figure) = &result[0].kind else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(only_image(figure).path.to_string(), "a.png");
    assert!(only_image(figure).width.is_none());
    assert!(only_image(figure).height.is_none());
  }

  #[test]
  fn figure_accepts_image_with_only_width() {
    let arena = Bump::new();
    let source = r"\begin{figure}\image[width=80mm]{a.png}\end{figure}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::Figure(figure) = &result[0].kind else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert!((only_image(figure).width.expect("width 指定あり").to_mm() - 80.0).abs() < 1e-4);
    assert!(only_image(figure).height.is_none());
  }

  #[test]
  fn figure_rejects_unknown_opt_key() {
    let arena = Bump::new();
    let source = r"\begin{figure}[foo=1]\image[width=1mm, height=1mm]{a}\end{figure}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }

  #[test]
  fn image_captures_dpi_and_downsample() {
    let arena = Bump::new();
    let source = r"\begin{figure}\image[width=80mm, dpi=600, downsample=false]{a.png}\end{figure}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::Figure(figure) = &result[0].kind else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(only_image(figure).dpi, Some(600));
    assert_eq!(only_image(figure).downsample, Some(false));
  }

  #[test]
  fn image_records_format_from_extension() {
    let arena = Bump::new();
    let source = r"\begin{figure}\image{a.SVG}\end{figure}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::Figure(figure) = &result[0].kind else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(only_image(figure).format, ImageFormat::Svg);
  }

  #[test]
  fn image_rejects_unsupported_extension_at_the_image_command() {
    let arena = Bump::new();
    let source = r"\begin{figure}\image{a.gif}\end{figure}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    let Err(EvalError::UnsupportedImageFormat { path, span }) = result else {
      panic!("UnsupportedImageFormat が期待されます: {result:?}");
    };
    assert_eq!(path, "a.gif");
    let command = r"\image{a.gif}";
    assert_eq!(span, SourceSpan::from((source.find(command).unwrap(), command.len())));
  }

  #[test]
  fn image_rejects_raster_only_options_on_svg() {
    for (option, expected_key) in [("dpi=300", "dpi"), ("downsample=false", "downsample")] {
      let arena = Bump::new();
      let source = format!(r"\begin{{figure}}\image[{option}]{{a.svg}}\end{{figure}}");
      let cst = test_support::parse_cst(&source, &arena).unwrap();

      let result = evaluate_children_to_hir(&source, cst);

      assert!(
        matches!(
          &result,
          Err(EvalError::ImageOptionNotApplicable { key, format: ImageFormat::Svg, applies_to, .. })
            if key == expected_key && applies_to == "PNG / JPEG"
        ),
        "{option}: {result:?}"
      );
    }
  }

  #[test]
  fn image_records_page_on_pdf() {
    let arena = Bump::new();
    let source = r"\begin{figure}\image[page=2]{a.pdf}\end{figure}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::Figure(figure) = &result[0].kind else {
      panic!("Figure が期待されます: {:?}", result[0]);
    };
    assert_eq!(only_image(figure).format, ImageFormat::Pdf);
    assert_eq!(only_image(figure).page, Some(2));
  }

  #[test]
  fn image_rejects_page_on_raster_and_svg() {
    for (path, format) in [
      ("a.png", ImageFormat::Png),
      ("a.jpg", ImageFormat::Jpeg),
      ("a.svg", ImageFormat::Svg),
    ] {
      let arena = Bump::new();
      let source = format!(r"\begin{{figure}}\image[page=1]{{{path}}}\end{{figure}}");
      let cst = test_support::parse_cst(&source, &arena).unwrap();

      let result = evaluate_children_to_hir(&source, cst);

      assert!(
        matches!(
          &result,
          Err(EvalError::ImageOptionNotApplicable { key, format: actual, applies_to, .. })
            if key == "page" && *actual == format && applies_to == "PDF"
        ),
        "{path}: {result:?}"
      );
    }
  }

  #[test]
  fn image_rejects_raster_only_options_on_pdf() {
    for (option, expected_key) in [("dpi=300", "dpi"), ("downsample=false", "downsample")] {
      let arena = Bump::new();
      let source = format!(r"\begin{{figure}}\image[{option}]{{a.pdf}}\end{{figure}}");
      let cst = test_support::parse_cst(&source, &arena).unwrap();

      let result = evaluate_children_to_hir(&source, cst);

      assert!(
        matches!(
          &result,
          Err(EvalError::ImageOptionNotApplicable { key, format: ImageFormat::Pdf, applies_to, .. })
            if key == expected_key && applies_to == "PNG / JPEG"
        ),
        "{option}: {result:?}"
      );
    }
  }

  #[test]
  fn image_rejects_page_zero() {
    let arena = Bump::new();
    let source = r"\begin{figure}\image[page=0]{a.pdf}\end{figure}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "page"), "{result:?}");
  }

  #[test]
  fn image_rejects_zero_dpi() {
    let arena = Bump::new();
    let source = r"\begin{figure}\image[dpi=0]{a.png}\end{figure}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "dpi"));
  }

  #[test]
  fn image_rejects_zero_width() {
    // 描画寸法 0 は krilla が受け付けないので、描画段まで運ばずここで弾く
    let arena = Bump::new();
    let source = r"\begin{figure}\image[width=0mm]{a.png}\end{figure}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "width"));
  }

  #[test]
  fn image_rejects_negative_height() {
    let arena = Bump::new();
    let source = r"\begin{figure}\image[height=-5mm]{a.png}\end{figure}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "height"));
  }

  /// `source` を文書のトップレベルで評価する
  #[expect(
    clippy::unwrap_in_result,
    reason = "構文エラーはテストの組み立て誤りで評価の結果ではないので、panic させてテストを落とす必要がある"
  )]
  fn evaluate(source: &str) -> Result<Vec<HirNode>, EvalError> {
    let arena = Bump::new();
    let cst = test_support::parse_cst(source, &arena).unwrap();
    return evaluate_children_to_hir(source, cst);
  }

  /// 評価結果の先頭の図
  fn first_figure(nodes: &[HirNode]) -> &HirFigure {
    let HirNodeKind::Figure(figure) = &nodes[0].kind else {
      panic!("Figure が期待されます: {:?}", nodes[0]);
    };
    return figure;
  }

  /// 副図の並びを持つ図の `per_row`・副キャプションの位置・副図列
  fn subfigures_of(figure: &HirFigure) -> (NonZeroU32, CaptionPosition, &[HirSubfigure]) {
    let FigureBody::Subfigures {
      per_row,
      caption_position,
      items,
    } = &figure.body
    else {
      panic!("副図の並びが期待されます: {figure:?}");
    };
    return (*per_row, *caption_position, items);
  }

  /// `source` の中で `needle` が最初に現れる位置から始まる長さ `len` の span
  fn span_at(source: &str, needle: &str, len: usize) -> SourceSpan {
    return SourceSpan::from((source.find(needle).expect("needle はソースにあるはず"), len));
  }

  #[test]
  fn subfigures_are_collected_in_source_order() {
    let source = r"\begin{figure}\begin{subfigure}[label=fig:a]\image{a.svg}\caption{A}\end{subfigure}\begin{subfigure}[label=fig:b]\image{b.svg}\caption{B}\end{subfigure}\caption{全体}\end{figure}";

    let nodes = evaluate(source).unwrap();

    let figure = first_figure(&nodes);
    let (per_row, position, items) = subfigures_of(figure);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].label.as_deref(), Some("fig:a"));
    assert_eq!(items[1].label.as_deref(), Some("fig:b"));
    assert_eq!(items[0].image.path.to_string(), "a.svg");
    assert_eq!(items[1].image.path.to_string(), "b.svg");
    assert!(items[0].caption.is_some());
    assert_eq!(per_row.get(), 2);
    assert_eq!(position, CaptionPosition::Bottom);
    assert_eq!(figure.caption_position, CaptionPosition::Bottom);
    assert_eq!(figure.images().count(), 2);
    assert_eq!(figure.captions().count(), 3);
  }

  #[test]
  fn per_row_is_recorded() {
    let source = r"\begin{figure}[per_row=1]\begin{subfigure}\image{a.svg}\end{subfigure}\begin{subfigure}\image{b.svg}\end{subfigure}\end{figure}";

    let nodes = evaluate(source).unwrap();

    assert_eq!(subfigures_of(first_figure(&nodes)).0.get(), 1);
  }

  #[test]
  fn single_subfigure_is_accepted() {
    let source = r"\begin{figure}\begin{subfigure}\image{a.svg}\end{subfigure}\end{figure}";

    let nodes = evaluate(source).unwrap();

    let (per_row, _, items) = subfigures_of(first_figure(&nodes));
    assert_eq!(items.len(), 1);
    assert_eq!(per_row.get(), 1);
  }

  #[test]
  fn subcaption_before_image_yields_top() {
    let source = r"\begin{figure}\caption{全体}\begin{subfigure}\caption{A}\image{a.svg}\end{subfigure}\begin{subfigure}\caption{B}\image{b.svg}\end{subfigure}\end{figure}";

    let nodes = evaluate(source).unwrap();

    let figure = first_figure(&nodes);
    assert_eq!(subfigures_of(figure).1, CaptionPosition::Top);
    assert_eq!(figure.caption_position, CaptionPosition::Top);
  }

  #[test]
  fn subfigure_without_caption_does_not_decide_position() {
    let source = r"\begin{figure}\begin{subfigure}\image{a.svg}\end{subfigure}\begin{subfigure}\caption{B}\image{b.svg}\end{subfigure}\end{figure}";

    let nodes = evaluate(source).unwrap();

    assert_eq!(subfigures_of(first_figure(&nodes)).1, CaptionPosition::Top);
  }

  #[test]
  fn subfigure_outside_figure_is_rejected() {
    for source in [
      r"\begin{subfigure}\image{a.svg}\end{subfigure}",
      r"\begin{quote}\begin{subfigure}\image{a.svg}\end{subfigure}\end{quote}",
    ] {
      let result = evaluate(source);

      let Err(EvalError::SubfigureOutsideFigure { span }) = result else {
        panic!("SubfigureOutsideFigure が期待されます: {source}: {result:?}");
      };
      let environment = r"\begin{subfigure}\image{a.svg}\end{subfigure}";
      assert_eq!(span, span_at(source, environment, environment.len()), "{source}");
    }
  }

  #[test]
  fn nested_subfigure_is_rejected() {
    let source = r"\begin{figure}\begin{subfigure}\image{a.svg}\begin{subfigure}\image{b.svg}\end{subfigure}\end{subfigure}\end{figure}";

    let result = evaluate(source);

    assert!(
      matches!(&result, Err(EvalError::UnexpectedContentInEnvironment { env, .. }) if env == "subfigure"),
      "{result:?}"
    );
  }

  #[test]
  fn subfigure_without_image_is_rejected() {
    let source = r"\begin{figure}\begin{subfigure}\caption{A}\end{subfigure}\end{figure}";

    let result = evaluate(source);

    assert!(
      matches!(&result, Err(EvalError::MissingEnvironmentArgument { name, .. }) if name == "subfigure"),
      "{result:?}"
    );
  }

  #[test]
  fn subfigure_with_two_images_or_captions_is_rejected() {
    for (body, duplicated) in [
      (r"\image{a.svg}\image{b.svg}", "image"),
      (r"\image{a.svg}\caption{A}\caption{B}", "caption"),
    ] {
      let source = format!(r"\begin{{figure}}\begin{{subfigure}}{body}\end{{subfigure}}\end{{figure}}");

      let result = evaluate(&source);

      assert!(
        matches!(&result, Err(EvalError::DuplicateCommandInEnvironment { env, name, .. })
          if env == "subfigure" && name == duplicated),
        "{body}: {result:?}"
      );
    }
  }

  #[test]
  fn image_and_subfigure_mixed_is_rejected() {
    let subfigure = r"\begin{subfigure}\image{b.svg}\end{subfigure}";
    for (source, later, later_len) in [
      (format!(r"\begin{{figure}}\image{{a.svg}}{subfigure}\end{{figure}}"), subfigure, subfigure.len()),
      (
        format!(r"\begin{{figure}}{subfigure}\image{{a.svg}}\end{{figure}}"),
        r"\image{a.svg}",
        r"\image{a.svg}".len(),
      ),
    ] {
      let result = evaluate(&source);

      let Err(EvalError::FigureImageWithSubfigures { span }) = result else {
        panic!("FigureImageWithSubfigures が期待されます: {source}: {result:?}");
      };
      assert_eq!(span, span_at(&source, later, later_len), "{source}");
    }
  }

  #[test]
  fn figure_caption_between_subfigures_is_rejected() {
    let source = r"\begin{figure}\begin{subfigure}\image{a.svg}\end{subfigure}\caption{全体}\begin{subfigure}\image{b.svg}\end{subfigure}\end{figure}";

    let result = evaluate(source);

    let Err(EvalError::FigureCaptionBetweenSubfigures { span }) = result else {
      panic!("FigureCaptionBetweenSubfigures が期待されます: {result:?}");
    };
    assert_eq!(span, span_at(source, r"\caption{全体}", r"\caption{全体}".len()));
  }

  #[test]
  fn mixed_subcaption_positions_are_rejected() {
    let source = r"\begin{figure}\begin{subfigure}\image{a.svg}\caption{A}\end{subfigure}\begin{subfigure}\caption{B}\image{b.svg}\end{subfigure}\end{figure}";

    let result = evaluate(source);

    let Err(EvalError::SubcaptionPositionMixed { span }) = result else {
      panic!("SubcaptionPositionMixed が期待されます: {result:?}");
    };
    assert_eq!(span, span_at(source, r"\caption{B}", r"\caption{B}".len()));
  }

  #[test]
  fn per_row_without_subfigures_is_rejected() {
    let source = r"\begin{figure}[per_row=2]\image{a.svg}\end{figure}";

    let result = evaluate(source);

    let Err(EvalError::PerRowWithoutSubfigures { span }) = result else {
      panic!("PerRowWithoutSubfigures が期待されます: {result:?}");
    };
    assert_eq!(span, SourceSpan::from((0, source.len())));
  }

  #[test]
  fn per_row_zero_and_fraction_are_rejected() {
    for value in ["0", "1.5"] {
      let source =
        format!(r"\begin{{figure}}[per_row={value}]\begin{{subfigure}}\image{{a.svg}}\end{{subfigure}}\end{{figure}}");

      let result = evaluate(&source);

      assert!(
        matches!(&result, Err(EvalError::InvalidOptArgValue { key, .. }) if key == "per_row"),
        "{value}: {result:?}"
      );
    }
  }

  #[test]
  fn subfigure_rejects_unknown_key() {
    let source = r"\begin{figure}\begin{subfigure}[width=1mm]\image{a.svg}\end{subfigure}\end{figure}";

    let result = evaluate(source);

    assert!(matches!(&result, Err(EvalError::UnknownOptArgKey { key, .. }) if key == "width"), "{result:?}");
  }
}
