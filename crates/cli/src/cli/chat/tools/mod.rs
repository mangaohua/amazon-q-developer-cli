pub mod custom_tool;
pub mod execute_bash;
pub mod fs_read;
pub mod fs_write;
pub mod gh_issue;
pub mod thinking;
pub mod use_aws;
pub mod web_browse;

use std::collections::HashMap;
use std::io::Write;
use std::path::{
    Path,
    PathBuf,
};

use crossterm::style::Stylize;
use custom_tool::CustomTool;
use execute_bash::ExecuteBash;
use eyre::Result;
use fs_read::FsRead;
use fs_write::FsWrite;
use gh_issue::GhIssue;
use serde::{
    Deserialize,
    Serialize,
};
use thinking::Thinking;
use use_aws::UseAws;
use web_browse::WebBrowse;

use super::consts::MAX_TOOL_RESPONSE_SIZE;
use super::util::images::RichImageBlocks;
use crate::platform::Context;

/// Represents an executable tool use.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Tool {
    FsRead(FsRead),
    FsWrite(FsWrite),
    ExecuteBash(ExecuteBash),
    UseAws(UseAws),
    Custom(CustomTool),
    GhIssue(GhIssue),
    Thinking(Thinking),
    WebBrowse(WebBrowse),
}

impl Tool {
    /// The display name of a tool
    pub fn display_name(&self) -> String {
        match self {
            Tool::FsRead(_) => "fs_read",
            Tool::FsWrite(_) => "fs_write",
            Tool::ExecuteBash(_) => "execute_bash",
            Tool::UseAws(_) => "use_aws",
            Tool::Custom(custom_tool) => &custom_tool.name,
            Tool::GhIssue(_) => "gh_issue",
            Tool::Thinking(_) => "thinking (prerelease)",
            Tool::WebBrowse(_) => "web_browse",
        }
        .to_owned()
    }

    /// Whether or not the tool should prompt the user to accept before [Self::invoke] is called.
    pub fn requires_acceptance(&self, _ctx: &Context) -> bool {
        match self {
            Tool::FsRead(_) => false,
            Tool::FsWrite(_) => true,
            Tool::ExecuteBash(execute_bash) => execute_bash.requires_acceptance(),
            Tool::UseAws(use_aws) => use_aws.requires_acceptance(),
            Tool::Custom(_) => true,
            Tool::GhIssue(_) => false,
            Tool::Thinking(_) => false,
            Tool::WebBrowse(_) => false, // Web browsing is generally safe, but could be made configurable
        }
    }

    /// Invokes the tool asynchronously
    pub async fn invoke(&self, context: &Context, updates: &mut impl Write) -> Result<InvokeOutput> {
        match self {
            Tool::FsRead(fs_read) => fs_read.invoke(context, updates).await,
            Tool::FsWrite(fs_write) => fs_write.invoke(context, updates).await,
            Tool::ExecuteBash(execute_bash) => execute_bash.invoke(updates).await,
            Tool::UseAws(use_aws) => use_aws.invoke(context, updates).await,
            Tool::Custom(custom_tool) => custom_tool.invoke(context, updates).await,
            Tool::GhIssue(gh_issue) => gh_issue.invoke(updates).await,
            Tool::Thinking(think) => think.invoke(updates).await,
            Tool::WebBrowse(web_browse) => web_browse.invoke(context, updates).await,
        }
    }

    /// Queues up a tool's intention in a human readable format
    pub async fn queue_description(&self, ctx: &Context, updates: &mut impl Write) -> Result<()> {
        match self {
            Tool::FsRead(fs_read) => fs_read.queue_description(ctx, updates).await,
            Tool::FsWrite(fs_write) => fs_write.queue_description(ctx, updates),
            Tool::ExecuteBash(execute_bash) => execute_bash.queue_description(updates),
            Tool::UseAws(use_aws) => use_aws.queue_description(updates),
            Tool::Custom(custom_tool) => custom_tool.queue_description(updates),
            Tool::GhIssue(gh_issue) => gh_issue.queue_description(updates),
            Tool::Thinking(thinking) => thinking.queue_description(updates),
            Tool::WebBrowse(web_browse) => web_browse.queue_description(updates),
        }
    }

    /// Validates the tool with the arguments supplied
    pub async fn validate(&mut self, ctx: &Context) -> Result<()> {
        match self {
            Tool::FsRead(fs_read) => fs_read.validate(ctx).await,
            Tool::FsWrite(fs_write) => fs_write.validate(ctx).await,
            Tool::ExecuteBash(execute_bash) => execute_bash.validate(ctx).await,
            Tool::UseAws(use_aws) => use_aws.validate(ctx).await,
            Tool::Custom(custom_tool) => custom_tool.validate(ctx).await,
            Tool::GhIssue(gh_issue) => gh_issue.validate(ctx).await,
            Tool::Thinking(think) => think.validate(ctx).await,
            Tool::WebBrowse(web_browse) => web_browse.validate(ctx).await,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolPermission {
    pub trusted: bool,
}

#[derive(Debug, Clone)]
/// Holds overrides for tool permissions.
/// Tools that do not have an associated ToolPermission should use
/// their default logic to determine to permission.
pub struct ToolPermissions {
    // We need this field for any stragglers
    pub trust_all: bool,
    pub permissions: HashMap<String, ToolPermission>,
}

impl ToolPermissions {
    pub fn new(capacity: usize) -> Self {
        Self {
            trust_all: false,
            permissions: HashMap::with_capacity(capacity),
        }
    }

    pub fn is_trusted(&self, tool_name: &str) -> bool {
        self.trust_all || self.permissions.get(tool_name).is_some_and(|perm| perm.trusted)
    }

    /// Returns a label to describe the permission status for a given tool.
    pub fn display_label(&self, tool_name: &str) -> String {
        if self.has(tool_name) || self.trust_all {
            if self.is_trusted(tool_name) {
                format!("  {}", "trusted".dark_green().bold())
            } else {
                format!("  {}", "not trusted".dark_grey())
            }
        } else {
            self.default_permission_label(tool_name)
        }
    }

    pub fn trust_tool(&mut self, tool_name: &str) {
        self.permissions
            .insert(tool_name.to_string(), ToolPermission { trusted: true });
    }

    pub fn untrust_tool(&mut self, tool_name: &str) {
        self.trust_all = false;
        self.permissions
            .insert(tool_name.to_string(), ToolPermission { trusted: false });
    }

    pub fn reset(&mut self) {
        self.trust_all = false;
        self.permissions.clear();
    }

    pub fn reset_tool(&mut self, tool_name: &str) {
        self.trust_all = false;
        self.permissions.remove(tool_name);
    }

    pub fn has(&self, tool_name: &str) -> bool {
        self.permissions.contains_key(tool_name)
    }

    /// Provide default permission labels for the built-in set of tools.
    // This "static" way avoids needing to construct a tool instance.
    fn default_permission_label(&self, tool_name: &str) -> String {
        let label = match tool_name {
            "fs_read" => "trusted".dark_green().bold(),
            "fs_write" => "not trusted".dark_grey(),
            "execute_bash" => "trust read-only commands".dark_grey(),
            "use_aws" => "trust read-only commands".dark_grey(),
            "report_issue" => "trusted".dark_green().bold(),
            "thinking" => "trusted (prerelease)".dark_green().bold(),
            "web_browse" => "trusted".dark_green().bold(),
            _ if self.trust_all => "trusted".dark_grey().bold(),
            _ => "not trusted".dark_grey(),
        };

        format!("{} {label}", "*".reset())
    }
}

/// A tool specification to be sent to the model as part of a conversation. Maps to
/// [BedrockToolSpecification].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    #[serde(alias = "inputSchema")]
    pub input_schema: InputSchema,
    #[serde(skip_serializing, default = "tool_origin")]
    pub tool_origin: ToolOrigin,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum ToolOrigin {
    Native,
    McpServer(String),
}

impl<'de> Deserialize<'de> for ToolOrigin {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s == "native___" {
            Ok(ToolOrigin::Native)
        } else {
            Ok(ToolOrigin::McpServer(s))
        }
    }
}

impl Serialize for ToolOrigin {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ToolOrigin::Native => serializer.serialize_str("native___"),
            ToolOrigin::McpServer(server) => serializer.serialize_str(server),
        }
    }
}

impl std::fmt::Display for ToolOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolOrigin::Native => write!(f, "Built-in"),
            ToolOrigin::McpServer(server) => write!(f, "{} (MCP)", server),
        }
    }
}

fn tool_origin() -> ToolOrigin {
    ToolOrigin::Native
}

#[derive(Debug, Clone)]
pub struct QueuedTool {
    pub id: String,
    pub name: String,
    pub accepted: bool,
    pub tool: Tool,
}

/// The schema specification describing a tool's fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputSchema(pub serde_json::Value);

/// The output received from invoking a [Tool].
#[derive(Debug, Default)]
pub struct InvokeOutput {
    pub output: OutputKind,
}

impl InvokeOutput {
    pub fn as_str(&self) -> &str {
        match &self.output {
            OutputKind::Text(s) => s.as_str(),
            OutputKind::Json(j) => j.as_str().unwrap_or_default(),
            OutputKind::Images(_) => "",
        }
    }
}

#[non_exhaustive]
#[derive(Debug)]
pub enum OutputKind {
    Text(String),
    Json(serde_json::Value),
    Images(RichImageBlocks),
}

impl Default for OutputKind {
    fn default() -> Self {
        Self::Text(String::new())
    }
}

/// Performs tilde expansion and other required sanitization modifications for handling tool use
/// path arguments.
///
/// Required since path arguments are defined by the model.
#[allow(dead_code)]
pub fn sanitize_path_tool_arg(ctx: &Context, path: impl AsRef<Path>) -> PathBuf {
    let mut res = PathBuf::new();
    // Expand `~` only if it is the first part.
    let mut path = path.as_ref().components();
    match path.next() {
        Some(p) if p.as_os_str() == "~" => {
            res.push(ctx.env().home().unwrap_or_default());
        },
        Some(p) => res.push(p),
        None => return res,
    }
    for p in path {
        res.push(p);
    }
    // For testing scenarios, we need to make sure paths are appropriately handled in chroot test
    // file systems since they are passed directly from the model.
    ctx.fs().chroot_path(res)
}

/// Converts `path` to a relative path according to the current working directory `cwd`.
fn absolute_to_relative(cwd: impl AsRef<Path>, path: impl AsRef<Path>) -> Result<PathBuf> {
    let cwd = cwd.as_ref().canonicalize()?;
    let path = path.as_ref().canonicalize()?;
    let mut cwd_parts = cwd.components().peekable();
    let mut path_parts = path.components().peekable();

    // Skip common prefix
    while let (Some(a), Some(b)) = (cwd_parts.peek(), path_parts.peek()) {
        if a == b {
            cwd_parts.next();
            path_parts.next();
        } else {
            break;
        }
    }

    // ".." for any uncommon parts, then just append the rest of the path.
    let mut relative = PathBuf::new();
    for _ in cwd_parts {
        relative.push("..");
    }
    for part in path_parts {
        relative.push(part);
    }

    Ok(relative)
}

/// Small helper for formatting the path as a relative path, if able.
fn format_path(cwd: impl AsRef<Path>, path: impl AsRef<Path>) -> String {
    absolute_to_relative(cwd, path.as_ref())
        .map(|p| p.to_string_lossy().to_string())
        // If we have three consecutive ".." then it should probably just stay as an absolute path.
        .map(|p| {
            if p.starts_with("../../..") {
                path.as_ref().to_string_lossy().to_string()
            } else {
                p
            }
        })
        .unwrap_or(path.as_ref().to_string_lossy().to_string())
}

fn supports_truecolor(ctx: &Context) -> bool {
    // Simple override to disable truecolor since shell_color doesn't use Context.
    !ctx.env().get("Q_DISABLE_TRUECOLOR").is_ok_and(|s| !s.is_empty())
        && shell_color::get_color_support().contains(shell_color::ColorSupport::TERM24BIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::EnvProvider;

    #[tokio::test]
    async fn test_tilde_path_expansion() {
        let ctx = Context::builder().with_test_home().await.unwrap().build_fake();

        let actual = sanitize_path_tool_arg(&ctx, "~");
        assert_eq!(
            actual,
            ctx.fs().chroot_path(ctx.env().home().unwrap()),
            "tilde should expand"
        );
        let actual = sanitize_path_tool_arg(&ctx, "~/hello");
        assert_eq!(
            actual,
            ctx.fs().chroot_path(ctx.env().home().unwrap().join("hello")),
            "tilde should expand"
        );
        let actual = sanitize_path_tool_arg(&ctx, "/~");
        assert_eq!(
            actual,
            ctx.fs().chroot_path("/~"),
            "tilde should not expand when not the first component"
        );
    }

    #[tokio::test]
    async fn test_format_path() {
        async fn assert_paths(cwd: &str, path: &str, expected: &str) {
            let ctx = Context::builder().with_test_home().await.unwrap().build_fake();
            let fs = ctx.fs();
            let cwd = sanitize_path_tool_arg(&ctx, cwd);
            let path = sanitize_path_tool_arg(&ctx, path);
            fs.create_dir_all(&cwd).await.unwrap();
            fs.create_dir_all(&path).await.unwrap();
            // Using `contains` since the chroot test directory will prefix the formatted path with a tmpdir
            // path.
            assert!(format_path(cwd, path).contains(expected));
        }
        assert_paths("/Users/testuser/src", "/Users/testuser/Downloads", "../Downloads").await;
        assert_paths(
            "/Users/testuser/projects/MyProject/src",
            "/Volumes/projects/MyProject/src",
            "/Volumes/projects/MyProject/src",
        )
        .await;
    }
}
