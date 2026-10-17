# Configuration

[Back to DeepHoney](../README.md)

## Settings and storage

API keys use the macOS Keychain under service `DeepHoney`. Non-secret settings use `~/Library/Application Support/DeepHoney/config.json`. Reports, evidence, and archived passages use `sessions.db` in that directory. Session content is local plaintext.

`DEEPHONEY_DATA_DIR` selects a separate data directory and a separate Keychain service, `DeepHoney:<directory>`. This supports isolated test profiles without modifying the default profile.

| Setting | Behavior |
|---|---|
| LLM provider | Anthropic Messages or an OpenAI-compatible chat-completions endpoint |
| Planner and writer | Default `claude-opus-5-5`; also used for gap checks, review, and report verification |
| Reader | Default `claude-sonnet-5-5`; extracts quoted claims from fetched text |
| Search | Brave, Exa, and Tavily accept keys. SearXNG accepts your instance URL. DuckDuckGo HTML is a best-effort fallback when none are configured. |
| Optional credentials | GitHub code search and CourtListener require tokens. SEC EDGAR is enabled when a contact is configured. |
| Contact | Included in the clearnet User-Agent, including SEC requests |
| Page budget | 1–1,000 attempts, default 160. Failed reads and archive allowances consume the budget. |
| Built-in Tor | Off by default. Required for Dark web mode. No external Tor daemon or SOCKS proxy is required. |

The plan displays upper bounds for search calls and page attempts. These are not token-cost or dollar estimates. Models must support the requested JSON responses. Provider quotas and endpoint compatibility still apply.

## Headless research

The CLI reads configuration from environment variables and writes Markdown to stdout and JSON progress events to stderr.

```sh
cargo run -p engine --example cli -- --help
cargo run -p engine --example cli -- "Your research question" --mode hidden --depth 3 --breadth 3 --pages 80 > report.md
```

Set `ANTHROPIC_API_KEY`, or set `OPENAI_BASE_URL`, `OPENAI_API_KEY`, and `MODEL` for a compatible endpoint. A local endpoint may allow an empty key. Search variables are `BRAVE_API_KEY`, `EXA_API_KEY`, `TAVILY_API_KEY`, and `SEARXNG_URL`. Optional variables are `GITHUB_TOKEN`, `COURTLISTENER_TOKEN`, and `DEEPHONEY_CONTACT`. Dark mode also requires `DEEPHONEY_TOR=1`. The CLI does not save sessions or read desktop Keychain settings.
