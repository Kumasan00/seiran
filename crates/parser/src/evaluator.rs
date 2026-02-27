use miette::Diagnostic;
use read_config::Config;
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
  #[must_use]
  pub fn new(config: &Config) -> Self {
    Evaluator {
      context: EvalContext::new(config.pdf.font_size),
    }
  }

  /// ブロックを評価してレイアウトノードに変換する
  ///
  /// # Errors
  ///
  /// 不明なコマンドや環境、引数の不足・過剰がある場合にエラーを返します
  pub fn evaluate_block(&mut self, block: Block) -> Result<Vec<LayoutNode>, EvalError> {
    let mut layout_nodes = Vec::new();
    for node in block {
      match node {
        Node::Text(text) => {
          Self::push_layout_node(&mut layout_nodes, LayoutNode::Text(text.to_string(), self.context.current_style));
        },
        Node::Command(command) => {
          let nodes = self.evaluate_command(command)?;
          for node in nodes {
            Self::push_layout_node(&mut layout_nodes, node);
          }
        },
        Node::Environment(environment) => {
          let nodes = self.evaluate_environment(&environment)?;
          for node in nodes {
            Self::push_layout_node(&mut layout_nodes, node);
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

#[cfg(test)]
mod tests {

  use super::*;

  fn default_style() -> Style {
    Style {
      font_size: 12.0,
      font_kind: FontKind::Serif,
    }
  }

  fn other_style() -> Style {
    Style {
      font_size: 24.0,
      font_kind: FontKind::SansSerif,
    }
  }

  // ========================================
  // push_layout_node: 空のVecへの追加
  // ========================================

  #[test]
  fn push_layout_node_text_to_empty_vec() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = Vec::new();
    let node = LayoutNode::Text("Hello".to_string(), default_style());

    // Act
    Evaluator::push_layout_node(&mut nodes, node);

    // Assert
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
      LayoutNode::Text(text, style) => {
        assert_eq!(text, "Hello");
        assert_eq!(*style, default_style());
      },
      _ => panic!("Textノードが期待されます"),
    }
  }

  #[test]
  fn push_layout_node_non_text_to_empty_vec() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = Vec::new();
    let node = LayoutNode::LineBreak;

    // Act
    Evaluator::push_layout_node(&mut nodes, node);

    // Assert
    assert_eq!(nodes.len(), 1);
    assert!(matches!(&nodes[0], LayoutNode::LineBreak));
  }

  // ========================================
  // push_layout_node: 同一スタイルのテキストマージ
  // ========================================

  #[test]
  fn push_layout_node_merges_same_style_text() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = vec![LayoutNode::Text("Hello".to_string(), default_style())];
    let node = LayoutNode::Text(" World".to_string(), default_style());

    // Act
    Evaluator::push_layout_node(&mut nodes, node);

    // Assert
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
      LayoutNode::Text(text, _) => assert_eq!(text, "Hello World"),
      _ => panic!("Textノードが期待されます"),
    }
  }

  #[test]
  fn push_layout_node_merges_multiple_same_style_texts() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = vec![LayoutNode::Text("A".to_string(), default_style())];

    // Act
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Text("B".to_string(), default_style()));
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Text("C".to_string(), default_style()));
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Text("D".to_string(), default_style()));

    // Assert
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
      LayoutNode::Text(text, _) => assert_eq!(text, "ABCD"),
      _ => panic!("Textノードが期待されます"),
    }
  }

  // ========================================
  // push_layout_node: 異なるスタイルのテキストは別ノード
  // ========================================

  #[test]
  fn push_layout_node_different_style_no_merge() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = vec![LayoutNode::Text("Hello".to_string(), default_style())];
    let node = LayoutNode::Text(" World".to_string(), other_style());

    // Act
    Evaluator::push_layout_node(&mut nodes, node);

    // Assert
    assert_eq!(nodes.len(), 2);
    match &nodes[0] {
      LayoutNode::Text(text, style) => {
        assert_eq!(text, "Hello");
        assert_eq!(*style, default_style());
      },
      _ => panic!("Textノードが期待されます"),
    }
    match &nodes[1] {
      LayoutNode::Text(text, style) => {
        assert_eq!(text, " World");
        assert_eq!(*style, other_style());
      },
      _ => panic!("Textノードが期待されます"),
    }
  }

  #[test]
  fn push_layout_node_different_font_size_no_merge() {
    // Arrange
    let style_a = Style {
      font_size: 10.0,
      font_kind: FontKind::Serif,
    };
    let style_b = Style {
      font_size: 14.0,
      font_kind: FontKind::Serif,
    };
    let mut nodes: Vec<LayoutNode> = vec![LayoutNode::Text("Small".to_string(), style_a)];

    // Act
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Text("Big".to_string(), style_b));

    // Assert
    assert_eq!(nodes.len(), 2);
  }

  #[test]
  fn push_layout_node_different_font_kind_no_merge() {
    // Arrange
    let style_a = Style {
      font_size: 12.0,
      font_kind: FontKind::Serif,
    };
    let style_b = Style {
      font_size: 12.0,
      font_kind: FontKind::Monospace,
    };
    let mut nodes: Vec<LayoutNode> = vec![LayoutNode::Text("Serif".to_string(), style_a)];

    // Act
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Text("Mono".to_string(), style_b));

    // Assert
    assert_eq!(nodes.len(), 2);
  }

  // ========================================
  // push_layout_node: 非テキストノードの後にテキスト追加
  // ========================================

  #[test]
  fn push_layout_node_text_after_non_text_no_merge() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = vec![LayoutNode::LineBreak];
    let node = LayoutNode::Text("After break".to_string(), default_style());

    // Act
    Evaluator::push_layout_node(&mut nodes, node);

    // Assert
    assert_eq!(nodes.len(), 2);
    assert!(matches!(&nodes[0], LayoutNode::LineBreak));
    match &nodes[1] {
      LayoutNode::Text(text, _) => assert_eq!(text, "After break"),
      _ => panic!("Textノードが期待されます"),
    }
  }

  #[test]
  fn push_layout_node_text_after_kern() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = vec![LayoutNode::Kern { point: 10.0 }];

    // Act
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Text("Text".to_string(), default_style()));

    // Assert
    assert_eq!(nodes.len(), 2);
    assert!(matches!(&nodes[0], LayoutNode::Kern { point } if (*point - 10.0).abs() < f32::EPSILON));
    assert!(matches!(&nodes[1], LayoutNode::Text(..)));
  }

  // ========================================
  // push_layout_node: 非テキストノード各種
  // ========================================

  #[test]
  fn push_layout_node_kern() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = Vec::new();

    // Act
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Kern { point: 5.0 });

    // Assert
    assert_eq!(nodes.len(), 1);
    assert!(matches!(&nodes[0], LayoutNode::Kern { point } if (*point - 5.0).abs() < f32::EPSILON));
  }

  #[test]
  fn push_layout_node_glue() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = Vec::new();

    // Act
    Evaluator::push_layout_node(
      &mut nodes,
      LayoutNode::Glue {
        natural: 10.0,
        stretch: 3.0,
        shrink: 2.0,
      },
    );

    // Assert
    assert_eq!(nodes.len(), 1);
    assert!(matches!(
      &nodes[0],
      LayoutNode::Glue { natural, stretch, shrink }
        if (*natural - 10.0).abs() < f32::EPSILON
          && (*stretch - 3.0).abs() < f32::EPSILON
          && (*shrink - 2.0).abs() < f32::EPSILON
    ));
  }

  #[test]
  fn push_layout_node_rule() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = Vec::new();

    // Act
    Evaluator::push_layout_node(
      &mut nodes,
      LayoutNode::Rule {
        width: 100.0,
        height: 1.0,
      },
    );

    // Assert
    assert_eq!(nodes.len(), 1);
    assert!(matches!(
      &nodes[0],
      LayoutNode::Rule { width, height }
        if (*width - 100.0).abs() < f32::EPSILON
          && (*height - 1.0).abs() < f32::EPSILON
    ));
  }

  #[test]
  fn push_layout_node_page_break() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = Vec::new();

    // Act
    Evaluator::push_layout_node(&mut nodes, LayoutNode::PageBreak);

    // Assert
    assert_eq!(nodes.len(), 1);
    assert!(matches!(&nodes[0], LayoutNode::PageBreak));
  }

  #[test]
  fn push_layout_node_vbox() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = Vec::new();
    let vbox = LayoutNode::VBox {
      children: vec![LayoutNode::Text("child".to_string(), default_style())],
      margin_bottom: 8.0,
    };

    // Act
    Evaluator::push_layout_node(&mut nodes, vbox);

    // Assert
    assert_eq!(nodes.len(), 1);
    assert!(matches!(&nodes[0], LayoutNode::VBox { margin_bottom, .. } if (*margin_bottom - 8.0).abs() < f32::EPSILON));
  }

  #[test]
  fn push_layout_node_hbox() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = Vec::new();
    let hbox = LayoutNode::HBox {
      children: vec![],
      width: Some(200.0),
    };

    // Act
    Evaluator::push_layout_node(&mut nodes, hbox);

    // Assert
    assert_eq!(nodes.len(), 1);
    assert!(matches!(&nodes[0], LayoutNode::HBox { width: Some(w), .. } if (*w - 200.0).abs() < f32::EPSILON));
  }

  // ========================================
  // push_layout_node: マージ境界の複合テスト
  // ========================================

  #[test]
  fn push_layout_node_merge_interrupted_by_non_text() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = Vec::new();

    // Act: Text → Text(マージ) → LineBreak → Text(マージされない)
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Text("A".to_string(), default_style()));
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Text("B".to_string(), default_style()));
    Evaluator::push_layout_node(&mut nodes, LayoutNode::LineBreak);
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Text("C".to_string(), default_style()));

    // Assert
    assert_eq!(nodes.len(), 3);
    match &nodes[0] {
      LayoutNode::Text(text, _) => assert_eq!(text, "AB"),
      _ => panic!("Textノードが期待されます"),
    }
    assert!(matches!(&nodes[1], LayoutNode::LineBreak));
    match &nodes[2] {
      LayoutNode::Text(text, _) => assert_eq!(text, "C"),
      _ => panic!("Textノードが期待されます"),
    }
  }

  #[test]
  fn push_layout_node_merge_interrupted_by_style_change() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = Vec::new();

    // Act: Text(default) → Text(default, マージ) → Text(other, 別ノード) → Text(other, マージ)
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Text("A".to_string(), default_style()));
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Text("B".to_string(), default_style()));
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Text("C".to_string(), other_style()));
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Text("D".to_string(), other_style()));

    // Assert
    assert_eq!(nodes.len(), 2);
    match &nodes[0] {
      LayoutNode::Text(text, style) => {
        assert_eq!(text, "AB");
        assert_eq!(*style, default_style());
      },
      _ => panic!("Textノードが期待されます"),
    }
    match &nodes[1] {
      LayoutNode::Text(text, style) => {
        assert_eq!(text, "CD");
        assert_eq!(*style, other_style());
      },
      _ => panic!("Textノードが期待されます"),
    }
  }

  #[test]
  fn push_layout_node_empty_text_merges() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = vec![LayoutNode::Text("Hello".to_string(), default_style())];

    // Act: 空文字列もマージされる
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Text(String::new(), default_style()));

    // Assert
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
      LayoutNode::Text(text, _) => assert_eq!(text, "Hello"),
      _ => panic!("Textノードが期待されます"),
    }
  }

  #[test]
  fn push_layout_node_non_text_nodes_do_not_merge() {
    // Arrange
    let mut nodes: Vec<LayoutNode> = Vec::new();

    // Act: 同種の非テキストノードは連続してもマージされない
    Evaluator::push_layout_node(&mut nodes, LayoutNode::LineBreak);
    Evaluator::push_layout_node(&mut nodes, LayoutNode::LineBreak);
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Kern { point: 5.0 });
    Evaluator::push_layout_node(&mut nodes, LayoutNode::Kern { point: 5.0 });

    // Assert
    assert_eq!(nodes.len(), 4);
  }
}
