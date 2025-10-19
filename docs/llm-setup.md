# LLM Setup Guide

Quick guide for setting up LLM integration with LM Studio.

## Prerequisites

- LM Studio installed
- Compatible model (Qwen3-30B-A3B or similar)
- At least 16GB RAM (32GB recommended for 30B models)

## Setup Steps

### 1. Install LM Studio

Download from https://lmstudio.ai/ and install.

### 2. Download Model

1. Open LM Studio
2. Navigate to "Discover" tab
3. Search for "qwen3-30b-a3b"
4. Download Q4_K_M quantization

### 3. Start Local Server

1. Click "Local Server" tab
2. Select downloaded model
3. Configure:
   - Port: 1234 (default)
   - Context Length: 20000 tokens
   - Max Tokens: 8000
4. Click "Start Server"

### 4. Configure drep

Update `config.yaml`:

```yaml
llm:
  enabled: true
  endpoint: http://localhost:1234/v1
  model: qwen3-30b-a3b
  temperature: 0.2
  max_tokens: 8000

  # Rate limiting
  max_concurrent_global: 5
  requests_per_minute: 60
  max_tokens_per_minute: 100000

  # Cache configuration
  cache:
    enabled: true
    directory: ~/.cache/drep/llm
    ttl_days: 30
    max_size_gb: 10.0

  # Circuit breaker (optional)
  circuit_breaker_threshold: 5
  circuit_breaker_timeout: 60
```

### 5. Verify Setup

Test your configuration:

```bash
drep scan owner/repo --show-metrics
```

Expected output:
- Code quality findings
- Missing docstring suggestions
- Metrics showing token usage
- Cache hit rate > 0% on second scan

## Remote LM Studio

For remote instances:

```yaml
llm:
  endpoint: https://lmstudio.example.com/v1
  api_key: ${LM_STUDIO_KEY}  # If authentication enabled
```

Set environment variable:
```bash
export LM_STUDIO_KEY=your-api-key-here
```

## Model Recommendations

| Model         | Size | RAM Required | Speed  | Quality   |
|---------------|------|--------------|--------|-----------|
| Qwen3-30B-A3B | 30B  | 32GB         | Medium | Excellent |
| Llama-3-70B   | 70B  | 64GB         | Slow   | Best      |
| Mistral-7B    | 7B   | 8GB          | Fast   | Good      |

## Troubleshooting

### Connection refused:
- Verify LM Studio is running
- Check endpoint URL matches
- Test: `curl http://localhost:1234/v1/models`

### Circuit breaker is OPEN:
- Wait for recovery timeout (default 60s)
- Check LM Studio logs
- Verify model is loaded

### Cache not working:
- Verify `cache.enabled: true`
- Check cache directory exists and is writable
- Ensure commit SHA is stable (don't scan uncommitted changes)

For more help, see the main README or create an issue.
