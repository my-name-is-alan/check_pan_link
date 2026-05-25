# Check Pan Link API

## Overview

`check-pan-link` is a Rust HTTP service built with Axum.

Default bind address:

- `APP_HOST=127.0.0.1`
- `APP_PORT=8080`

The Docker image overrides `APP_HOST` to `0.0.0.0` so the service is reachable from outside the container.

## Environment Variables

| Name | Required | Default | Description |
| --- | --- | --- | --- |
| `APP_HOST` | No | `127.0.0.1` | HTTP bind host |
| `APP_PORT` | No | `8080` | HTTP bind port |
| `CHECK_TIMEOUT_SECS` | No | `10` | Outbound link check timeout in seconds |
| `CORS_ALLOWED_ORIGIN` | No | unset | Optional CORS origin, supports `*` |
| `TELOXIDE_TOKEN` | No | unset | Telegram bot token used by `/telegram/webhook` |
| `TELEGRAM_WEBHOOK_SECRET` | No | unset | Optional secret checked against `x-telegram-bot-api-secret-token` |

## Docker Usage

Build image:

```bash
docker build -t check-pan-link .
```

Run container:

```bash
docker run --rm -p 8080:8080 check-pan-link
```

Run with Telegram support:

```bash
docker run --rm -p 8080:8080 \
  -e TELOXIDE_TOKEN=your_bot_token \
  -e TELEGRAM_WEBHOOK_SECRET=your_webhook_secret \
  check-pan-link
```

## Base URL

Examples below use:

```text
http://127.0.0.1:8080
```

## Endpoints

### `GET /healthz`

Health check endpoint.

#### Response

```json
{
  "status": "ok"
}
```

### `POST /api/check`

Checks a cloud-drive share link and returns normalized status information.

#### Request Body

```json
{
  "url": "https://115cdn.com/s/swfsfjg3h7i?password=l3a6"
}
```

#### Success Response

```json
{
  "original_url": "https://115cdn.com/s/swfsfjg3h7i?password=l3a6",
  "normalized_url": "https://115cdn.com/s/swfsfjg3h7i?password=l3a6",
  "status": "valid",
  "provider": "pan115",
  "reason": "share_available",
  "metadata": {
    "api_endpoint": "https://webapi.115.com/share/snap",
    "api_errno": 0,
    "api_http_status": 200,
    "api_state": true,
    "page_entry_errno_name": "ok",
    "page_entry_error_kind": "none",
    "receive_code": "l3a6",
    "receive_code_provided": true,
    "share_code": "swfsfjg3h7i",
    "share_state": 1,
    "share_state_label": "normal",
    "share_title": "记录的地平线 ログ・ホライズン 4K.60fps(2013)"
  }
}
```

#### Example: 115 expired share

```json
{
  "original_url": "https://115cdn.com/s/swhc5pf3fw0?password=rf10",
  "normalized_url": "https://115cdn.com/s/swhc5pf3fw0?password=rf10",
  "status": "invalid",
  "provider": "pan115",
  "reason": "share_expired",
  "metadata": {
    "api_endpoint": "https://webapi.115.com/share/snap",
    "api_errno": 0,
    "api_http_status": 200,
    "api_state": true,
    "forbid_reason": "链接已过期",
    "page_entry_errno_name": "ok",
    "page_entry_error_kind": "none",
    "share_code": "swhc5pf3fw0",
    "share_state": 7,
    "share_state_label": "expired",
    "share_title": "伊波拉病毒.mkv"
  }
}
```

#### Example: 115 missing access code

```json
{
  "original_url": "https://115cdn.com/s/swfsfjg3h7i",
  "normalized_url": "https://115cdn.com/s/swfsfjg3h7i",
  "status": "invalid",
  "provider": "pan115",
  "reason": "missing_receive_code",
  "metadata": {
    "api_endpoint": "https://webapi.115.com/share/snap",
    "api_errno": 4100012,
    "api_http_status": 200,
    "api_state": false,
    "is_access": 0,
    "page_entry_errno_name": "receive_code_required",
    "page_entry_error_kind": "receive_code",
    "page_entry_error_message": "请输入访问码",
    "receive_code_provided": false,
    "share_code": "swfsfjg3h7i"
  }
}
```

#### Example: 115 invalid access code

```json
{
  "original_url": "https://115cdn.com/s/swfsfjg3h7i?password=xxxx",
  "normalized_url": "https://115cdn.com/s/swfsfjg3h7i?password=xxxx",
  "status": "invalid",
  "provider": "pan115",
  "reason": "invalid_receive_code",
  "metadata": {
    "api_endpoint": "https://webapi.115.com/share/snap",
    "api_errno": 4100008,
    "api_http_status": 200,
    "api_state": false,
    "page_entry_errno_name": "receive_code_invalid",
    "page_entry_error_kind": "receive_code",
    "page_entry_error_message": "访问码错误",
    "receive_code": "xxxx",
    "receive_code_provided": true,
    "share_code": "swfsfjg3h7i"
  }
}
```

#### Example: 115 invalid share code

```json
{
  "original_url": "https://115cdn.com/s/swf12345678",
  "normalized_url": "https://115cdn.com/s/swf12345678",
  "status": "invalid",
  "provider": "pan115",
  "reason": "invalid_share_code",
  "metadata": {
    "api_endpoint": "https://webapi.115.com/share/snap",
    "api_errno": 990002,
    "api_errtype": "err",
    "api_http_status": 200,
    "api_state": false,
    "page_entry_errno_name": "share_code_invalid",
    "page_entry_error_kind": "share_code",
    "page_entry_error_message": "参数错误。",
    "receive_code_provided": false,
    "share_code": "swf12345678"
  }
}
```

#### Example: 115 share containing violation content

```json
{
  "original_url": "https://115cdn.com/s/swfck0r33y3?password=s548",
  "normalized_url": "https://115cdn.com/s/swfck0r33y3?password=s548",
  "status": "invalid",
  "provider": "pan115",
  "reason": "share_contains_violation",
  "metadata": {
    "api_endpoint": "https://webapi.115.com/share/snap",
    "api_errno": 0,
    "api_http_status": 200,
    "api_state": true,
    "have_vio_file": 1,
    "page_entry_errno_name": "ok",
    "page_entry_error_kind": "none",
    "receive_code": "s548",
    "receive_code_provided": true,
    "share_code": "swfck0r33y3",
    "share_state": 1,
    "share_state_label": "normal",
    "share_title": "丑陋的美国人-2010-[tmdb=32351]"
  }
}
```

#### Status Values

| Value | Meaning |
| --- | --- |
| `valid` | Link is confirmed usable |
| `invalid` | Link is confirmed unavailable or rejected |
| `unknown` | Service could not reach a stable conclusion |

#### Provider Values

| Value | Meaning |
| --- | --- |
| `pan115` | 115 share link |
| `pan189` | China Telecom Cloud share link |
| `pan123` | 123 Pan share link |
| `generic` | Fallback HTTP checker |

#### Common Error Response

```json
{
  "error": {
    "code": "invalid_url",
    "message": "not a url"
  }
}
```

#### Documented API Error Codes

| HTTP Status | Error Code | Meaning |
| --- | --- | --- |
| `400` | `invalid_url` | Request body URL is invalid |
| `400` | `unsupported_scheme` | URL scheme is not `http` or `https` |
| `401` | `telegram_secret_mismatch` | Telegram webhook secret does not match |
| `503` | `telegram_disabled` | `TELOXIDE_TOKEN` is not configured |
| `502` | `telegram_reply_failed` | Telegram reply operation failed |
| `500` | `http_client_error` | Internal HTTP client initialization failed |

### `POST /telegram/webhook`

Receives a Telegram `Update` payload and replies through the configured bot token.

#### Headers

Optional when `TELEGRAM_WEBHOOK_SECRET` is configured:

```text
x-telegram-bot-api-secret-token: your_webhook_secret
```

#### Request Body

The request body must be a Telegram `Update` JSON payload.

#### Success Response

```text
HTTP 200 OK
```

No JSON body is returned.
