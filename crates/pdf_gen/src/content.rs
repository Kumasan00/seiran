use parser::layout_engine::Item;
use pdf_writer::Content;

pub(crate) struct PDFContent {
  pub content: Content,
  pub annotations: Vec<Annotation>,
}

pub(crate) struct Annotation;

pub(crate) fn create_pdf_contents(_items: Vec<Item>) -> Vec<PDFContent> {
  vec![PDFContent {
    content: Content::new(),
    annotations: Vec::new(),
  }]
}
