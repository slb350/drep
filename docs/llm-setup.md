# LLM Setup Guide

Quick guide for setting up LLM integration with **LM Studio** (local models) or **AWS Bedrock** (cloud-hosted Claude models).

## Option 1: LM Studio (Local Models)

### Prerequisites

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

## Option 2: AWS Bedrock (Cloud-Hosted Claude Models)

### Prerequisites

- AWS account with Bedrock access
- AWS CLI configured or credentials in `~/.aws/credentials`
- Model access enabled in AWS Bedrock console

### Setup Steps

#### 1. Enable Model Access

1. Go to [AWS Bedrock Console](https://console.aws.amazon.com/bedrock/) (region: us-east-1 or your preferred region)
2. Navigate to **Model Access** in the left sidebar
3. Click **Modify model access**
4. Select Anthropic Claude models:
   - Claude Sonnet 4.5
   - Claude Haiku 4.5
   - Any other Claude models you want to use
5. Click **Save changes**
6. Wait for access to be granted (usually instant)

#### 2. Configure AWS Credentials

Bedrock uses the standard AWS credentials chain. Choose one method:

**Method A: AWS CLI Configuration**
```bash
aws configure
# Enter your AWS Access Key ID
# Enter your AWS Secret Access Key
# Default region: us-east-1
# Default output format: json
```

**Method B: Environment Variables**
```bash
export AWS_ACCESS_KEY_ID=your_access_key_id
export AWS_SECRET_ACCESS_KEY=your_secret_access_key
export AWS_DEFAULT_REGION=us-east-1
```

**Method C: Credentials File**
Create `~/.aws/credentials`:
```ini
[default]
aws_access_key_id = your_access_key_id
aws_secret_access_key = your_secret_access_key
region = us-east-1
```

#### 3. Configure drep

Update `config.yaml` to use Bedrock:

```yaml
llm:
  enabled: true
  provider: bedrock  # Required for Bedrock

  bedrock:
    region: us-east-1
    model: anthropic.claude-sonnet-4-5-20250929-v1:0  # See model IDs below

  # General LLM settings
  temperature: 0.2
  max_tokens: 4000

  # Rate limiting (optional, lower for Bedrock to avoid throttling)
  max_concurrent_global: 3
  requests_per_minute: 30
  max_tokens_per_minute: 50000

  # Cache configuration
  cache:
    enabled: true
    directory: ~/.cache/drep/llm
    ttl_days: 30
    max_size_gb: 10.0
```

#### 4. Verify Setup

Test your Bedrock configuration:

```bash
# Test with a simple scan
drep scan owner/repo --show-metrics
```

Expected output:
- Successful LLM initialization: "LLM backend: AWS Bedrock"
- Code quality findings
- Metrics showing token usage
- No authentication errors

### Supported Bedrock Models

**Claude Sonnet 4.5** (Recommended for most use cases)
```yaml
model: anthropic.claude-sonnet-4-5-20250929-v1:0
```
- Best balance of speed, cost, and quality
- Excellent for code review and documentation analysis
- Availability: global, us, eu, jp regions

**Claude Haiku 4.5** (Fast and cost-effective)
```yaml
model: anthropic.claude-haiku-4-5-20251001-v1:0
```
- Fastest response times
- Lower cost
- Good for simple code checks and docstring generation
- Availability: global, us, eu regions

**Global vs Regional Model IDs**
```yaml
# Regional model ID (default)
model: anthropic.claude-sonnet-4-5-20250929-v1:0

# Global model ID (for cross-region deployments)
model: global.anthropic.claude-sonnet-4-5-20250929-v1:0
```

### Bedrock Regions

| Region       | Code         | Sonnet 4.5 | Haiku 4.5 |
|--------------|--------------|------------|-----------|
| US East      | us-east-1    | ✅         | ✅        |
| US West      | us-west-2    | ✅         | ✅        |
| EU (Frankfurt) | eu-central-1 | ✅       | ✅        |
| Asia Pacific | ap-southeast-1 | ❌     | ❌        |

### Bedrock Troubleshooting

**AccessDeniedException:**
- Verify model access is enabled in AWS Bedrock console
- Check IAM permissions include `bedrock:InvokeModel`
- Ensure you're in the correct AWS region

**ThrottlingException:**
- Reduce `max_concurrent_global` (try 2-3)
- Lower `requests_per_minute` (try 20-30)
- Consider using Haiku 4.5 for faster throughput

**Invalid model ID:**
- Verify the model ID matches exactly (case-sensitive)
- Check model availability in your region
- Use correct format: `anthropic.claude-sonnet-4-5-20250929-v1:0`

**Credentials not found:**
- Run `aws configure` to set up credentials
- Check `~/.aws/credentials` file exists
- Verify AWS_ACCESS_KEY_ID environment variable is set

**Region unavailable:**
- Some Claude models aren't available in all regions
- Use us-east-1 for maximum model availability
- Check the region table above for model availability

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
