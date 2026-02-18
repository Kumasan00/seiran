use miette::Diagnostic;
use read_config_file::Config;
use thiserror::Error;
use types::FontKind;

use crate::parser::{Block, Node};

#[derive(Debug, Error, Diagnostic)]
#[allow(dead_code)]
pub enum EvalError {
  #[error("コマンドの引数が不足しています: {0}")]
  #[diagnostic(code(parser::eval::missing_command_argument), help("コマンドに必要な引数を確認してください"))]
  MissingCommandArgument(String),
  #[error("コマンドの余分な引数があります: {0}")]
  #[diagnostic(code(parser::eval::extra_command_argument), help("コマンドの引数数を確認してください"))]
  ExtraCommandArgument(String),
  #[error("コマンドの引数が不正です: {0}, {1}")]
  #[diagnostic(code(parser::eval::invalid_command_argument), help("引数の型や形式を確認してください"))]
  InvalidCommandArgument(String, String),
  #[error("不明なコマンドです: {0}")]
  #[diagnostic(code(parser::eval::unknown_command), help("コマンド名のスペルを確認してください"))]
  UnknownCommand(String),
  #[error("環境の引数が不足しています: {0}")]
  #[diagnostic(code(parser::eval::missing_environment_argument), help("環境に必要な引数を確認してください"))]
  MissingEnvironmentArgument(String),
  #[error("環境の余分な引数があります: {0}")]
  #[diagnostic(code(parser::eval::extra_environment_argument), help("環境の引数数を確認してください"))]
  ExtraEnvironmentArgument(String),
  #[error("不明な環境です: {0}")]
  #[diagnostic(code(parser::eval::unknown_environment), help("環境名のスペルを確認してください"))]
  UnknownEnvironment(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
  pub font_size: f32,
  pub font_kind: FontKind,
}

impl Style {
  fn new(font_size: f32) -> Self {
    Style {
      font_size,
      font_kind: FontKind::Serif,
    }
  }
}

/// レイアウトエンジンが処理する最小単位
#[derive(Debug, Clone)]
pub enum LayoutNode {
  /// スタイル付きテキスト
  Text(String, Style),
  /// 垂直方向のコンテナ (段落、セクションなど)
  VBox {
    children: Vec<LayoutNode>,
    margin_bottom: f32, // Glueの一種
  },
  /// 水平方向のコンテナ (行、インライン数式など)
  HBox {
    children: Vec<LayoutNode>,
    width: Option<f32>, // 固定幅の場合など
  },
  /// 画像や描画線など
  Rule {
    width: f32,
    height: f32,
  },
  Glue {
    natural: f32,
    stretch: f32,
    shrink: f32,
  },
  Kern {
    point: f32,
  },
  LineBreak,
  PageBreak,
}

#[derive(Debug)]
pub(crate) struct EvalContext {
  pub(crate) current_style: Style,
  pub(crate) part_num: u32,
  pub(crate) chapter_num: u32,
  pub(crate) section_num: u32,
  pub(crate) subsection_num: u32,
  pub(crate) paragraph_num: u32,
  pub(crate) subparagraph_num: u32,
}

impl EvalContext {
  fn new(font_size: f32) -> Self {
    EvalContext {
      current_style: Style::new(font_size),
      part_num: 0,
      chapter_num: 0,
      section_num: 0,
      subsection_num: 0,
      paragraph_num: 0,
      subparagraph_num: 0,
    }
  }
}

pub struct Evaluator {
  pub(crate) context: EvalContext,
}

impl Evaluator {
  pub fn new(config: &Config) -> Self {
    Evaluator {
      context: EvalContext::new(config.pdf.font_size),
    }
  }

  pub fn evaluate_block(&mut self, block: Block) -> Result<Vec<LayoutNode>, EvalError> {
    let mut layout_nodes = Vec::new();
    for node in block {
      match node {
        Node::Text(text) => {
          Self::push_layout_node(&mut layout_nodes, LayoutNode::Text(text.to_string(), self.context.current_style));
        },
        Node::Command(command) => {
          let node = self.evaluate_command(command)?;
          Self::push_layout_node(&mut layout_nodes, node);
        },
        Node::Environment(environment) => {
          let nodes = self.evaluate_environment(&environment)?;
          for node in nodes {
            layout_nodes.push(node);
          }
        },
        Node::InlineMath(_inline_math) => {
          // インライン数式の評価 (仮実装)
          // ここでは単純にテキストノードとして扱う例
          Self::push_layout_node(
            &mut layout_nodes,
            LayoutNode::Text("[InlineMath]".to_string(), self.context.current_style),
          );
        },
        Node::LineBreak => layout_nodes.push(LayoutNode::LineBreak),
        Node::ParagraphBreak => {
          layout_nodes.push(LayoutNode::LineBreak);
          layout_nodes.push(LayoutNode::Kern {
            point: self.context.current_style.font_size,
          });
        },
      }
    }
    return Ok(layout_nodes);
  }

  /// 連続する同一スタイルのテキストノードをマージしてから追加する
  fn push_layout_node(layout_nodes: &mut Vec<LayoutNode>, node: LayoutNode) {
    match node {
      LayoutNode::Text(text, style) => {
        if let Some(LayoutNode::Text(prev_text, prev_style)) = layout_nodes.last_mut()
          && *prev_style == style
        {
          prev_text.push_str(&text);
          return;
        }
        layout_nodes.push(LayoutNode::Text(text, style));
      },
      _ => {
        layout_nodes.push(node);
      },
    }
    return;
  }
}
