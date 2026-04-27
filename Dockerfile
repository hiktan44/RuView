# RuView Docker - Serves UI and WebSocket
FROM python:3.11-slim-bookworm

WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Install Python dependencies
COPY v1/requirements-lock.txt /app/requirements.txt
RUN pip install --no-cache-dir -r requirements.txt \
    && pip install --no-cache-dir websockets uvicorn fastapi aiohttp

# Copy application code
COPY v1/ /app/v1/
COPY ui/ /app/ui/

# Copy sensing modules
COPY v1/src/sensing/ /app/v1/src/sensing/

# Copy startup script
COPY docker/start.sh /app/start.sh
RUN chmod +x /app/start.sh

EXPOSE 8080
EXPOSE 8765

ENV PYTHONUNBUFFERED=1
ENV PYTHONDONTWRITEBYTECODE=1

CMD ["/app/start.sh"]
