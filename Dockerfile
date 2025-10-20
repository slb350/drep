FROM python:3.11-slim

WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get install -y \
    git \
    && rm -rf /var/lib/apt/lists/*

# Copy project files and install package
COPY . /app
RUN pip install --no-cache-dir .

# Create data directories
RUN mkdir -p /app/data /app/repos

# Expose port
EXPOSE 8000

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD python - <<'PY' || exit 1
import sys
try:
    import httpx
    r = httpx.get('http://localhost:8000/api/health', timeout=2.0)
    sys.exit(0 if r.status_code == 200 else 1)
except Exception:
    sys.exit(1)
PY

# Run server
CMD ["drep", "serve", "--host", "0.0.0.0", "--port", "8000"]
