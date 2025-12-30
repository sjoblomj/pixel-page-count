# Pixel Page Counter

A lightweight page view tracking service built with Rust and Axum.

## What it does

This service tracks page views using a 1x1 transparent GIF pixel and stores the data in a SQLite database. It's designed to be embedded in web pages for simple analytics without relying on third-party services.

## Endpoints

- **`GET /counter.gif?domain=<domain>&page=<page_name>`** - Returns a 1x1 transparent GIF and records the page view
- **`GET /stats.json`** - Returns analytics data in JSON format

## Usage

### Running the service

```bash
cargo run
```

The server will start on `http://0.0.0.0:8080`.

### Tracking page views

Embed the pixel in your HTML:

```html
<img src="http://localhost:8080/counter.gif?domain=example.com&page=/home" width="1" height="1" alt="" />
```

### Viewing analytics

```bash
curl http://localhost:8080/stats.json
```

Example response:

```json
{
  "summary": {
    "total_events": 42,
    "unique_pages": 5
  },
  "latest": [
    {
      "ts": 1765905585,
      "domain": "example.com",
      "page": "/home"
    }
  ]
}
```

#### Filtering by domain

You can optionally filter the analytics data by domain:

```bash
curl http://localhost:8080/stats.json?domain=example.com
```

This will return only the page views for the specified domain. If no domain parameter is provided, all page views are returned.

## Data Storage

Page views are stored in `data/analytics.db` (SQLite).
