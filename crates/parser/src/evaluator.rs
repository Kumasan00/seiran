use read_config_file::Config;
use thiserror::Error;

use crate::parser::{Block, Node};

#[derive(Debug, Error)]
pub enum EvalError {
  #[error("Missing argument for command: {0}")]
  MissingCommandArgument(String),
  #[error("extra argument for command: {0}")]
  ExtraCommandArgument(String),
  #[error("Invalid argument for command: {0}, {1}")]
  InvalidCommandArgument(String, String),
  #[error("Unknown command: {0}")]
  UnknownCommand(String),
  // #[error("Missing argument for environment: {0}")]
  // MissingEnvironmentArgument(String),
  #[error("extra argument for environment: {0}")]
  ExtraEnvironmentArgument(String),
  #[error("Unknown environment: {0}")]
  UnknownEnvironment(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum FontStyle {
  Serif,
  SerifBold,
  SerifItalic,
  SerifBoldItalic,
  SansSerif,
  SansSerifBold,
  SansSerifItalic,
  SansSerifBoldItalic,
  Monospace,
  MonospaceBold,
  MonospaceItalic,
  MonospaceBoldItalic,
  Math,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
  pub font_size: f32,
  pub font_type: FontStyle,
}

impl Style {
  fn new(font_size: f32) -> Self {
    Style {
      font_size,
      font_type: FontStyle::Serif,
    }
  }
}

/// レイアウトエンジンが処理する最小単位
#[derive(Debug, Clone)]
#[allow(dead_code)]
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
  ParagraphBreak,
  PageBreak,
}

#[derive(Debug)]
#[allow(dead_code)]
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
          // println!("Evaluated text node: {}", text);
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
        Node::ParagraphBreak => layout_nodes.push(LayoutNode::ParagraphBreak),
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
