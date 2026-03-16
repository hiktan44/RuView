#!/bin/bash
# RuView Startup Script
# Runs both WebSocket server and HTTP UI server

set -e

echo "Starting RuView servers..."

# Start WebSocket server in background
python -m v1.src.sensing.ws_server &
WS_PID=$!
echo "WebSocket server started on port 8765 (PID: $WS_PID)"

# Start HTTP server for UI
cd /app/ui
python -c "
import http.server
import socketserver
import os

PORT = 8080
Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({
    '.js': 'application/javascript',
    '.css': 'text/css',
    '.html': 'text/html',
    '.json': 'application/json',
    '.png': 'image/png',
    '.jpg': 'image/jpeg',
    '.svg': 'image/svg+xml',
})
print(f'UI Server running on port {PORT}')
with socketserver.TCPServer(('', PORT), Handler) as httpd:
    httpd.serve_forever()
" &
HTTP_PID=$!
echo "HTTP UI server started on port 8080 (PID: $HTTP_PID)"

echo "RuView is ready!"
echo "UI: http://localhost:8080"
echo "WebSocket: ws://localhost:8765"

# Wait for either process to exit
wait -n
