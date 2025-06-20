# Web Browse Tool Usage Examples

The `web_browse` tool allows Amazon Q CLI to fetch and analyze web content. Here are some usage examples:

## Basic Usage

```json
{
  "name": "web_browse",
  "args": {
    "url": "https://example.com"
  }
}
```

## Text-Only Extraction

```json
{
  "name": "web_browse",
  "args": {
    "url": "https://news.ycombinator.com",
    "text_only": true,
    "max_length": 10000
  }
}
```

## Custom Timeout and Length Limits

```json
{
  "name": "web_browse",
  "args": {
    "url": "https://slow-website.com",
    "timeout": 60,
    "max_length": 100000,
    "text_only": false
  }
}
```

## Features

- **URL Validation**: Only HTTP and HTTPS URLs are supported for security
- **Content Type Detection**: Automatically detects and reports content type
- **HTML Text Extraction**: Can extract clean text from HTML pages, removing scripts and styles
- **Content Length Limiting**: Prevents excessive memory usage with configurable limits
- **Timeout Control**: Configurable request timeout to handle slow websites
- **Security**: Only allows safe HTTP/HTTPS protocols

## Parameters

- `url` (required): The URL to browse (must be HTTP or HTTPS)
- `text_only` (optional, default: false): Extract only text content from HTML
- `max_length` (optional, default: 50000): Maximum content length in characters
- `timeout` (optional, default: 30): Request timeout in seconds

## Use Cases

1. **Content Analysis**: Fetch and analyze web articles or documentation
2. **Research**: Gather information from multiple web sources
3. **Monitoring**: Check website content or status
4. **Data Extraction**: Extract specific information from web pages

## Security Considerations

- Only HTTP and HTTPS protocols are allowed
- Content length is limited to prevent memory exhaustion
- Request timeouts prevent hanging connections
- No execution of JavaScript or other dynamic content
