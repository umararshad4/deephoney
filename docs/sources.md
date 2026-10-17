# Sources and network reference

[Back to DeepHoney](../README.md)

## Modes and sources

| Mode | Sources and behavior |
|---|---|
| Normal | Surface and deep sources, perspective planning, quoted claims, identifier pivots, gap checks, and evidence review |
| Hidden data | Normal sources plus Wayback discovery, archive recovery and historical diffs |
| Dark web | Hidden data sources plus filtered Ahmia discovery and onion page reads over embedded Arti |
| Adversarial | Hidden data sources plus a reserved counterevidence pass and a further review of contested evidence |

Adversarial mode does not imply Dark web mode. Only Dark web mode permits onion reads.

`Source::for_mode(mode, &cfg)` returns adapters enabled by the mode and credentials.

| Stratum | Adapter names | Constraints |
|---|---|---|
| Surface | `brave`, `exa`, `tavily`, `searxng`, `duckduckgo` | DuckDuckGo scraping can encounter bot challenges. |
| Deep | `openalex`, `semanticscholar`, `arxiv`, `crossref`, `hackernews`, `stackexchange` | Public scholarly and community endpoints; rate limits apply. |
| Deep | `sec_edgar`, `github_code`, `courtlistener` | SEC needs a configured contact. GitHub and CourtListener need tokens. See [configuration](configuration.md). |
| Deep | `crtsh` | Exact domain lookup in certificate transparency records. |
| Deep | `muckrock` | Public freedom-of-information requests and released records. |
| Deep | `telegram` | Public previews of a named `@channel` or `t.me/channel`, with optional search words. No global Telegram search. |
| Deep | `commoncrawl` | Exact URL or domain lookup in the latest index. Results lead to current pages, not decoded archived WARC content. |
| Buried | `wayback` | Exact URL or domain lookup returning dated HTML or plain-text captures. |
| Dark | `ahmia` | Filtered onion discovery; requires Dark web mode and Tor opt-in. |

Archive recovery and historical diffs also use `ghost.rs`. Local and onion URLs are excluded from public archive queries. The retired Memento TimeTravel service is not used; its operator documents the shutdown in [Memento's service notice](https://mementoweb.org/about/).

## Dark web transport

Enable **Built-in Tor** in Settings, select **Dark web**, and create a plan that includes `ahmia`. The engine validates v3 onion addresses and admits hosts only when Ahmia's filtered index returns them. Embedded [Arti](https://arti.torproject.org/guides/connecting-to-onion/) bootstraps lazily when the first onion page is read.

Only onion page reads use Tor. Public search requests, Ahmia discovery, and configured model APIs use direct connections. Evidence sent to a remote model therefore leaves Tor. This application does not provide whole-session anonymity.

Onion requests omit the configured contact, cookies, authorization, and referrer. Redirects stay within independently admitted onion hosts. Clearnet-to-onion redirects are rejected before DNS, and onion requests have no direct-network fallback. Automatic navigation from an onion page to a clearnet link is disabled.

Both transports cap HTML/plain-text bodies at 2 MiB. Clearnet also reads text-based PDFs up to 12 MiB and supports optional isolated JavaScript rendering. Dark mode disables browser rendering. The onion transport requires an explicit supported content type and validates TLS certificates for HTTPS. It does not execute JavaScript, submit forms, authenticate to sites, or download PDFs, images, or binaries. Tor bootstrap and reads have timeouts. An unavailable onion service produces a warning; discovery does not guarantee reachability. Keep Arti updated as the Tor network changes.
