# API Documentation

Base URL:

- Development backend direct: `http://localhost:1103`
- Production through Myriad proxy: same-origin, for example `https://yourdomain.com`

All endpoint paths are the same in both modes.

## Table of Contents

- [Health Check](#health-check)
- [Configuration](#configuration)
- [Platforms](#platforms)
- [Profiles](#profiles)
- [Analysis](#analysis)

## Health Check

### GET /health

Check if the API server is running.

**Response:**

```json
{
  "status": "ok",
  "schema_version": 1,
  "version": "v0.1.0",
  "db_connected": true,
  "migrations_applied": true,
  "storage_writable": true,
  "uptime_seconds": 123,
  "database_connected": true
}
```

**Status Codes:**

- `200 OK` - Service is healthy

---

## Configuration

### GET /api/config

Retrieve current system configuration.

**Response:**

```json
{
  "platforms": [
    {
      "name": "GitHub",
      "enabled": true,
      "has_token": true
    },
    {
      "name": "Twitter",
      "enabled": false,
      "has_token": false
    }
  ],
  "ai_config": {
    "provider": "Google Gemini",
    "model": "gemini-2.0-flash-exp",
    "enabled": true
  },
  "fetch_config": {
    "auto_fetch": false,
    "interval_hours": 24
  }
}
```

**Status Codes:**

- `200 OK` - Configuration retrieved successfully

### POST /api/config

Update system configuration.

**Request Body:**

```json
{
  "platforms": [
    {
      "name": "GitHub",
      "enabled": true,
      "has_token": true
    }
  ],
  "ai_config": {
    "provider": "Google Gemini",
    "model": "gemini-2.0-flash-exp",
    "enabled": true
  },
  "fetch_config": {
    "auto_fetch": true,
    "interval_hours": 12
  }
}
```

**Response:**

```json
{
  "success": true,
  "message": "Configuration updated successfully"
}
```

**Status Codes:**

- `200 OK` - Configuration updated successfully
- `400 Bad Request` - Invalid configuration data
- `500 Internal Server Error` - Failed to update configuration

---

## Platforms

### GET /api/platforms

List all supported platforms.

**Response:**

```json
{
  "platforms": [
    {
      "id": 1,
      "name": "GitHub",
      "enabled": true,
      "icon": "github"
    },
    {
      "id": 2,
      "name": "Twitter",
      "enabled": false,
      "icon": "twitter"
    }
  ]
}
```

**Status Codes:**

- `200 OK` - Platforms retrieved successfully

---

## Profiles

### GET /api/profiles

Get all fetched user profiles.

**Response:**

```json
{
  "profiles": [
    {
      "id": 1,
      "platform": "GitHub",
      "username": "octocat",
      "display_name": "The Octocat",
      "avatar_url": "https://github.com/images/octocat.png",
      "bio": "GitHub mascot",
      "location": "San Francisco",
      "website": "https://github.com/octocat",
      "fetched_at": "2025-10-30T10:00:00Z",
      "stats": {
        "followers": 1000,
        "following": 100,
        "repositories": 50
      }
    }
  ],
  "total": 1
}
```

**Status Codes:**

- `200 OK` - Profiles retrieved successfully
- `404 Not Found` - No profiles found

### POST /api/fetch

Trigger manual data fetch from platforms.

**Request Body:**

```json
{
  "platforms": ["github", "twitter"],
  "force": false
}
```

**Parameters:**

- `platforms` (optional): Array of platform names to fetch. If omitted, fetches from all enabled platforms.
- `force` (optional): If true, fetches even if recently fetched. Default: false.

**Response:**

```json
{
  "success": true,
  "message": "Fetch triggered successfully",
  "job_id": 123
}
```

**Status Codes:**

- `200 OK` - Fetch triggered successfully
- `400 Bad Request` - Invalid platform names
- `429 Too Many Requests` - Rate limit exceeded
- `500 Internal Server Error` - Fetch failed

---

## Analysis

### GET /api/analysis

Get AI analysis results.

**Query Parameters:**

- `type` (optional): Filter by analysis type (e.g., "profile_summary", "skill_extraction")
- `limit` (optional): Number of results to return. Default: 10
- `offset` (optional): Pagination offset. Default: 0

**Response:**

```json
{
  "analysis": [
    {
      "id": 1,
      "type": "profile_summary",
      "result": {
        "summary": "Active software developer with strong presence on GitHub...",
        "key_skills": ["Python", "JavaScript", "Rust"],
        "activity_level": "high",
        "interests": ["Open Source", "Web Development"]
      },
      "ai_model": "gemini-2.0-flash-exp",
      "created_at": "2025-10-30T12:00:00Z"
    }
  ],
  "total": 1
}
```

**Status Codes:**

- `200 OK` - Analysis results retrieved successfully
- `404 Not Found` - No analysis results found

### POST /api/analysis

Trigger AI analysis of profile data.

**Request Body:**

```json
{
  "type": "profile_summary",
  "profile_ids": [1, 2, 3],
  "options": {
    "detail_level": "comprehensive",
    "include_recommendations": true
  }
}
```

**Parameters:**

- `type`: Type of analysis to perform
  - `profile_summary` - General summary of user's digital presence
  - `skill_extraction` - Extract and categorize skills
  - `personality_analysis` - Analyze personality traits
  - `content_analysis` - Analyze posted content
  - `trend_analysis` - Identify activity trends
- `profile_ids` (optional): Specific profile IDs to analyze. If omitted, analyzes all profiles.
- `options` (optional): Additional options for the analysis

**Response:**

```json
{
  "success": true,
  "message": "Analysis triggered successfully",
  "analysis_id": 456,
  "estimated_time_seconds": 30
}
```

**Status Codes:**

- `200 OK` - Analysis triggered successfully
- `400 Bad Request` - Invalid analysis type or parameters
- `402 Payment Required` - Insufficient API credits
- `429 Too Many Requests` - Rate limit exceeded
- `500 Internal Server Error` - Analysis failed

---

## Error Responses

All endpoints may return error responses in the following format:

```json
{
  "error": "Error message describing what went wrong"
}
```

**Common Status Codes:**

- `400 Bad Request` - Invalid request parameters
- `404 Not Found` - Resource not found
- `429 Too Many Requests` - Rate limit exceeded
- `500 Internal Server Error` - Server error

---

## Rate Limiting

API endpoints are subject to rate limiting to prevent abuse:

- Configuration endpoints: 10 requests per minute
- Fetch endpoints: 5 requests per minute
- Analysis endpoints: 3 requests per minute
- Other endpoints: 60 requests per minute

Rate limit headers are included in responses:

```
X-RateLimit-Limit: 60
X-RateLimit-Remaining: 59
X-RateLimit-Reset: 1698672000
```

---

## Authentication

Currently, the API does not require authentication as it's designed for single-user deployment. Future versions may include:

- API key authentication
- OAuth 2.0 support
- JWT tokens for multi-user scenarios

---

## Webhooks (Planned)

Future versions will support webhooks for real-time notifications:

- `fetch.completed` - When a data fetch completes
- `analysis.completed` - When an AI analysis completes
- `error.occurred` - When an error occurs in background jobs

---

## CORS

The API supports CORS for frontend access. Default allowed origins:

- `http://localhost:1102` (development)
- `http://localhost:1103` (development)

Configure additional origins in the `.env` file:

```env
CORS_ORIGINS=http://localhost:1102,https://yourdomain.com
```

---

## Development

### Testing with cURL

**Health check:**

```bash
curl http://localhost:1103/health
```

**Get configuration:**

```bash
curl http://localhost:1103/api/config
```

**Trigger fetch:**

```bash
curl -X POST http://localhost:1103/api/fetch \
  -H "Content-Type: application/json" \
  -d '{"platforms": ["github"]}'
```

### Testing with PowerShell

**Health check:**

```powershell
Invoke-RestMethod -Uri "http://localhost:1103/health"
```

**Get configuration:**

```powershell
Invoke-RestMethod -Uri "http://localhost:1103/api/config"
```

**Trigger fetch:**

```powershell
$body = @{
    platforms = @("github")
} | ConvertTo-Json

Invoke-RestMethod -Uri "http://localhost:1103/api/fetch" `
  -Method Post `
  -ContentType "application/json" `
  -Body $body
```

---

## Client Libraries

### TypeScript/JavaScript

The frontend includes a TypeScript client in `frontend/src/lib/api.ts`:

```typescript
import {
  fetchConfig,
  updateConfig,
  reloadSystemConfig,
} from "../frontend/src/lib/api";
// 或在前端工程内使用相对路径 / path alias 导入。
// 注：平台抓取与分析请走 services/api 或后端 REST；
// lib/api 目前主要封装配置/备份/权限/语音状态等。

// Get configuration
const config = await fetchConfig();

// Update configuration
await updateConfig(config);

// Reload system config after env changes
await reloadSystemConfig();
```

### Rust

For Rust applications, you can use the `reqwest` crate:

```rust
use reqwest::Client;

let client = Client::new();
let response = client
    .get("http://localhost:1103/api/config")
    .send()
    .await?;

let config: serde_json::Value = response.json().await?;
```

---

## Changelog

### v0.1.0 (2025-10-30)

- Initial API implementation
- Basic CRUD endpoints for configuration
- Platform listing endpoint
- Fetch trigger endpoint
- Analysis endpoints (stubs)

---

## 📚 Practical Examples & Usage Guide

This section provides practical examples and common workflows for using the Myriad API.

### Common Workflows

#### 1. Initial Setup Workflow

```bash
# Step 1: Check API health
curl http://localhost:1103/health

# Step 2: Get current configuration
curl http://localhost:1103/api/config

# Step 3: Update configuration with API keys
curl -X POST http://localhost:1103/api/config \
  -H "Content-Type: application/json" \
  -d '{
    "github_token": "ghp_xxxxxxxxxxxx",
    "steam_api_key": "xxxxxxxxxxxxx",
    "gemini_api_key": "xxxxxxxxxxxxx"
  }'

# Step 4: Verify configuration
curl http://localhost:1103/api/config
```

#### 2. Data Collection Workflow

```bash
# Fetch data from all platforms
curl -X POST http://localhost:1103/api/fetch

# Fetch from specific platforms only
curl -X POST http://localhost:1103/api/fetch \
  -H "Content-Type: application/json" \
  -d '{
    "platforms": ["GitHub", "Steam"]
  }'

# Force refresh (ignore cache)
curl -X POST http://localhost:1103/api/fetch \
  -H "Content-Type: application/json" \
  -d '{
    "force": true
  }'
```

#### 3. Analysis Workflow

```bash
# Trigger AI analysis
curl -X POST http://localhost:1103/api/analysis \
  -H "Content-Type: application/json" \
  -d '{
    "type": "profile_summary",
    "options": {
      "detail_level": "comprehensive"
    }
  }'

# Get analysis results
curl http://localhost:1103/api/analysis

# Get specific analysis type
curl "http://localhost:1103/api/analysis?type=profile_summary&limit=5"
```

### Platform-Specific Examples

#### Bilibili API

```bash
# Get user complete information
curl "http://localhost:1103/api/bilibili/user?uid=123456"

# Get user basic info
curl "http://localhost:1103/api/bilibili/user/123456"

# Get favorites list
curl "http://localhost:1103/api/bilibili/favorites/123456"

# Get bangumi (anime) list
curl "http://localhost:1103/api/bilibili/bangumi/123456?bangumi_type=1"

# Get all bangumi/cinema follows
curl "http://localhost:1103/api/bilibili/bangumi/all/123456"
```

**Bangumi Types:**

- `1` - Anime
- `2` - Movies
- `3` - Documentaries
- `4` - Chinese animation
- `5` - TV Shows

#### Bangumi API

```bash
# Get user complete information and collections
curl "http://localhost:1103/api/bangumi/user?username=example"

# Get user basic info
curl "http://localhost:1103/api/bangumi/user/example"

# Get current user info with an access token
curl "http://localhost:1103/api/bangumi/me?access_token=YOUR_TOKEN"

# Get collections list
curl "http://localhost:1103/api/bangumi/collections/example"

# Optional: include access token and custom User-Agent for private collections
curl "http://localhost:1103/api/bangumi/collections/example?access_token=YOUR_TOKEN&user_agent=haru%2FMyriad"
```

#### GitHub API

```bash
# Get user profile
curl "http://localhost:1103/api/github/user/username"

# Get user repositories
curl "http://localhost:1103/api/github/repos/username"

# Get repository details
curl "http://localhost:1103/api/github/repo/owner/repo-name"
```

#### Steam API

```bash
# Get user profile
curl "http://localhost:1103/api/steam/user/76561198012345678"

# Get owned games
curl "http://localhost:1103/api/steam/games/76561198012345678"

# Get recent games
curl "http://localhost:1103/api/steam/recent/76561198012345678"
```

### JavaScript/TypeScript Examples

#### Using Fetch API

```javascript
// Get configuration
async function getConfig() {
  const response = await fetch("http://localhost:1103/api/config");
  const config = await response.json();
  console.log(config);
}

// Update configuration
async function updateConfig(newConfig) {
  const response = await fetch("http://localhost:1103/api/config", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(newConfig),
  });
  return response.json();
}

// Trigger data fetch (raw REST example — not a lib/api helper)
async function triggerFetch(platforms = null) {
  const body = platforms ? { platforms } : {};
  const response = await fetch("http://localhost:1103/api/fetch", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  return response.json();
}

// Get analysis results
async function getAnalysis(type = null, limit = 10) {
  const params = new URLSearchParams();
  if (type) params.append("type", type);
  params.append("limit", limit.toString());

  const response = await fetch(`http://localhost:1103/api/analysis?${params}`);
  return response.json();
}
```

#### React Component Example

```typescript
import { useState, useEffect } from "react";

function ConfigPanel() {
  const [config, setConfig] = useState(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetch("http://localhost:1103/api/config")
      .then((res) => res.json())
      .then((data) => {
        setConfig(data);
        setLoading(false);
      });
  }, []);

  const handleSave = async (newConfig) => {
    const response = await fetch("http://localhost:1103/api/config", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(newConfig),
    });

    if (response.ok) {
      alert("Configuration saved!");
      setConfig(newConfig);
    }
  };

  if (loading) return <div>Loading...</div>;

  return (
    <div>
      <h2>Configuration</h2>
      <pre>{JSON.stringify(config, null, 2)}</pre>
      {/* Add form controls here */}
    </div>
  );
}
```

### Python Examples

```python
import requests

BASE_URL = "http://localhost:1103"

# Get configuration
def get_config():
    response = requests.get(f"{BASE_URL}/api/config")
    return response.json()

# Update configuration
def update_config(config):
    response = requests.post(
        f"{BASE_URL}/api/config",
        json=config
    )
    return response.json()

# Trigger fetch
def trigger_fetch(platforms=None, force=False):
    data = {"force": force}
    if platforms:
        data["platforms"] = platforms

    response = requests.post(
        f"{BASE_URL}/api/fetch",
        json=data
    )
    return response.json()

# Get analysis
def get_analysis(analysis_type=None, limit=10):
    params = {"limit": limit}
    if analysis_type:
        params["type"] = analysis_type

    response = requests.get(
        f"{BASE_URL}/api/analysis",
        params=params
    )
    return response.json()

# Example usage
if __name__ == "__main__":
    # Get current config
    config = get_config()
    print("Current config:", config)

    # Fetch data
    result = trigger_fetch(platforms=["GitHub", "Steam"])
    print("Fetch result:", result)

    # Get analysis
    analysis = get_analysis(analysis_type="profile_summary")
    print("Analysis:", analysis)
```

### Error Handling

#### JavaScript

```javascript
async function safeFetch(url, options = {}) {
  try {
    const response = await fetch(url, options);

    if (!response.ok) {
      const error = await response.json();
      throw new Error(error.error || "Request failed");
    }

    return await response.json();
  } catch (error) {
    console.error("API Error:", error.message);
    // Handle error appropriately
    throw error;
  }
}

// Usage
try {
  const config = await safeFetch("http://localhost:1103/api/config");
  console.log(config);
} catch (error) {
  alert(`Failed to load configuration: ${error.message}`);
}
```

#### Python

```python
def safe_api_call(func):
    def wrapper(*args, **kwargs):
        try:
            return func(*args, **kwargs)
        except requests.exceptions.ConnectionError:
            print("Error: Cannot connect to API server")
        except requests.exceptions.Timeout:
            print("Error: Request timed out")
        except requests.exceptions.HTTPError as e:
            print(f"HTTP Error: {e.response.status_code}")
            print(e.response.json())
        except Exception as e:
            print(f"Unexpected error: {str(e)}")
        return None
    return wrapper

@safe_api_call
def get_config():
    response = requests.get(f"{BASE_URL}/api/config", timeout=5)
    response.raise_for_status()
    return response.json()
```

### Rate Limiting

The API implements rate limiting to prevent abuse:

- **Default limit**: 100 requests per 15 minutes per IP
- **Fetch endpoint**: 10 requests per hour
- **Analysis endpoint**: 20 requests per hour

#### Handle Rate Limits

```javascript
async function fetchWithRetry(url, options = {}, maxRetries = 3) {
  for (let i = 0; i < maxRetries; i++) {
    try {
      const response = await fetch(url, options);

      if (response.status === 429) {
        // Rate limited
        const retryAfter = response.headers.get("Retry-After") || 60;
        console.log(`Rate limited. Retrying after ${retryAfter}s...`);
        await new Promise((resolve) => setTimeout(resolve, retryAfter * 1000));
        continue;
      }

      return response;
    } catch (error) {
      if (i === maxRetries - 1) throw error;
      await new Promise((resolve) => setTimeout(resolve, 1000 * (i + 1)));
    }
  }
}
```

### Webhooks (Future)

> **Note:** Webhook support is planned for future releases.

Expected webhook events:

- `fetch.completed` - Data fetch completed
- `analysis.completed` - Analysis completed
- `config.updated` - Configuration changed
- `error.occurred` - Error occurred

---

## 🔒 Authentication (Coming Soon)

Future versions will include authentication:

```bash
# Login to get token
curl -X POST http://localhost:1103/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username": "admin", "password": "password"}'

# Response
{
  "token": "eyJhbGc...",
  "expires_in": 3600
}

# Use token in requests
curl http://localhost:1103/api/config \
  -H "Authorization: Bearer eyJhbGc..."
```

---

## 📖 Additional Resources

- **Frontend API Client**: See `frontend/src/lib/api.ts` for TypeScript client implementation
- **Backend Source**: See `backend/src/api/` for endpoint implementations
- **Deployment Guide**: [Docker Deployment](deployment/DOCKER_DEPLOYMENT.md)
- **Architecture**: [System Architecture](development/ARCHITECTURE.md)

---

**API Version**: 0.1.0

**Last Updated**: 2026-07-03

**Base URL**: `http://localhost:1103` in development; same-origin through the Myriad proxy in production.
