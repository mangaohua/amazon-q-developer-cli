use std::io::Write;
use std::time::Duration;

use eyre::Result;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use url::Url;

use super::{InvokeOutput, OutputKind};
use crate::platform::Context;

/// Tool for browsing web pages and extracting their content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebBrowse {
    /// The URL to browse
    pub url: String,
    /// Optional: Extract only text content (default: false)
    #[serde(default)]
    pub text_only: bool,
    /// Optional: Maximum content length to return (default: 50000 characters)
    #[serde(default = "default_max_length")]
    pub max_length: usize,
    /// Optional: Timeout in seconds (default: 30)
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

fn default_max_length() -> usize {
    50000
}

fn default_timeout() -> u64 {
    30
}

impl WebBrowse {
    pub async fn invoke(&self, _ctx: &Context, updates: &mut impl Write) -> Result<InvokeOutput> {
        writeln!(updates, "🌐 Browsing: {}", self.url)?;
        
        // Validate URL
        let url = Url::parse(&self.url)
            .map_err(|e| eyre::eyre!("Invalid URL '{}': {}", self.url, e))?;
        
        // Only allow HTTP and HTTPS schemes for security
        if !matches!(url.scheme(), "http" | "https") {
            return Err(eyre::eyre!("Only HTTP and HTTPS URLs are supported"));
        }

        // Create HTTP client with timeout and user agent
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(self.timeout))
            .build()?;

        // Set up headers
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("Amazon Q CLI Web Browser/1.0"),
        );

        // Make the request
        writeln!(updates, "📡 Fetching content...")?;
        let response = client
            .get(&self.url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| eyre::eyre!("Failed to fetch URL: {}", e))?;

        // Check if the request was successful
        if !response.status().is_success() {
            return Err(eyre::eyre!(
                "HTTP request failed with status: {}",
                response.status()
            ));
        }

        // Get content type
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|ct| ct.to_str().ok())
            .unwrap_or("unknown")
            .to_string(); // Convert to owned String

        writeln!(updates, "📄 Content-Type: {}", content_type)?;

        // Get the response body
        let body = response
            .text()
            .await
            .map_err(|e| eyre::eyre!("Failed to read response body: {}", e))?;

        // Process content based on type and user preferences
        let processed_content = if self.text_only || content_type.contains("text/html") {
            self.extract_text_content(&body)?
        } else {
            body
        };

        // Truncate if necessary
        let final_content = if processed_content.len() > self.max_length {
            writeln!(
                updates,
                "⚠️  Content truncated to {} characters (original: {} characters)",
                self.max_length,
                processed_content.len()
            )?;
            format!(
                "{}\n\n[... Content truncated. Original length: {} characters ...]",
                &processed_content[..self.max_length],
                processed_content.len()
            )
        } else {
            processed_content
        };

        writeln!(updates, "✅ Successfully fetched {} characters", final_content.len())?;

        Ok(InvokeOutput {
            output: OutputKind::Text(final_content),
        })
    }

    pub fn queue_description(&self, updates: &mut impl Write) -> Result<()> {
        if self.text_only {
            writeln!(updates, "Browse {} (text only)", self.url)?;
        } else {
            writeln!(updates, "Browse {}", self.url)?;
        }
        Ok(())
    }

    pub async fn validate(&mut self, _ctx: &Context) -> Result<()> {
        // Validate URL format
        Url::parse(&self.url)
            .map_err(|e| eyre::eyre!("Invalid URL format '{}': {}", self.url, e))?;

        // Validate max_length
        if self.max_length == 0 {
            return Err(eyre::eyre!("max_length must be greater than 0"));
        }

        // Validate timeout
        if self.timeout == 0 {
            return Err(eyre::eyre!("timeout must be greater than 0"));
        }

        Ok(())
    }

    /// Extract text content from HTML
    fn extract_text_content(&self, html: &str) -> Result<String> {
        let mut text = String::new();
        let mut in_tag = false;
        let mut in_script_or_style = false;
        let mut current_tag = String::new();
        
        let chars: Vec<char> = html.chars().collect();
        let mut i = 0;
        
        while i < chars.len() {
            let ch = chars[i];
            
            if ch == '<' {
                in_tag = true;
                current_tag.clear();
                
                // Look ahead to determine tag type
                let mut j = i + 1;
                let mut is_closing = false;
                
                // Skip whitespace
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                
                // Check if it's a closing tag
                if j < chars.len() && chars[j] == '/' {
                    is_closing = true;
                    j += 1;
                }
                
                // Read tag name
                while j < chars.len() && (chars[j].is_alphabetic() || chars[j].is_numeric()) {
                    current_tag.push(chars[j].to_ascii_lowercase());
                    j += 1;
                }
                
                if is_closing {
                    if current_tag == "script" || current_tag == "style" {
                        in_script_or_style = false;
                    }
                } else {
                    if current_tag == "script" || current_tag == "style" {
                        in_script_or_style = true;
                    }
                }
            } else if ch == '>' {
                in_tag = false;
            } else if !in_tag && !in_script_or_style {
                if ch == '\n' || ch == '\r' {
                    if !text.ends_with('\n') && !text.is_empty() {
                        text.push('\n');
                    }
                } else if ch.is_whitespace() {
                    if !text.ends_with(' ') && !text.is_empty() {
                        text.push(' ');
                    }
                } else {
                    text.push(ch);
                }
            }
            
            i += 1;
        }
        
        // Clean up extra whitespace
        let lines: Vec<&str> = text
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect();
        
        Ok(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text_content() {
        let web_browse = WebBrowse {
            url: "https://example.com".to_string(),
            text_only: true,
            max_length: 1000,
            timeout: 30,
        };

        let html = r#"
            <html>
                <head>
                    <title>Test Page</title>
                    <script>console.log('test');</script>
                    <style>body { color: red; }</style>
                </head>
                <body>
                    <h1>Hello World</h1>
                    <p>This is a test paragraph.</p>
                    <div>
                        <span>Nested content</span>
                    </div>
                </body>
            </html>
        "#;

        let result = web_browse.extract_text_content(html).unwrap();
        
        // Should extract text content and exclude script/style content
        assert!(result.contains("Test Page"));
        assert!(result.contains("Hello World"));
        assert!(result.contains("This is a test paragraph."));
        assert!(result.contains("Nested content"));
        assert!(!result.contains("console.log"));
        assert!(!result.contains("color: red"));
    }

    #[tokio::test]
    async fn test_url_validation() {
        let mut web_browse = WebBrowse {
            url: "invalid-url".to_string(),
            text_only: false,
            max_length: 1000,
            timeout: 30,
        };

        let ctx = Context::builder()
            .build_fake();

        // Should fail validation for invalid URL
        assert!(web_browse.validate(&ctx).await.is_err());

        // Should pass validation for valid URL
        web_browse.url = "https://example.com".to_string();
        assert!(web_browse.validate(&ctx).await.is_ok());
    }

    #[tokio::test]
    async fn test_parameter_validation() {
        let ctx = Context::builder()
            .build_fake();

        // Test max_length validation
        let mut web_browse = WebBrowse {
            url: "https://example.com".to_string(),
            text_only: false,
            max_length: 0,
            timeout: 30,
        };
        assert!(web_browse.validate(&ctx).await.is_err());

        // Test timeout validation
        web_browse.max_length = 1000;
        web_browse.timeout = 0;
        assert!(web_browse.validate(&ctx).await.is_err());

        // Test valid parameters
        web_browse.timeout = 30;
        assert!(web_browse.validate(&ctx).await.is_ok());
    }
}
