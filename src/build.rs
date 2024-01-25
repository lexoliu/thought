use liquid::Template;

use crate::{
    utils::{read_to_string, render_markdown},
    workspace::Workspace,
    Result,
};

pub struct BuildResource {
    pub article_template: Template,
    pub index_template: Template,
    pub footer: String,
}

impl BuildResource {
    pub fn load(workspace: &Workspace) -> Result<Self> {
        let index_template = read_to_string(workspace.template_path().join("index.html"))?;
        let article_template = read_to_string(workspace.template_path().join("article.html"))?;
        let parser = liquid::ParserBuilder::with_stdlib().build().unwrap();
        let index_template = parser.parse(&index_template)?;
        let article_template = parser.parse(&article_template)?;
        let footer = render_markdown(read_to_string(workspace.path().join("footer.md"))?);
        Ok(Self {
            article_template,
            index_template,
            footer,
        })
    }
}
