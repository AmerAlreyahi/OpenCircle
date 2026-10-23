# AI setup

OpenCircle has no AI of its own and no account. Ask AI, Explain and AI
translation use a provider you choose, with your own key. Nothing is sent until
you ask a question.

## Providers

Two kinds, set in **Settings → AI**:

| Provider | Covers |
|---|---|
| Anthropic | Claude (default model `claude-sonnet-5-5`) |
| OpenAI-compatible | OpenAI, Gemini (its OpenAI-compatible endpoint), OpenRouter, Ollama, LM Studio, or any server that speaks the OpenAI chat-completions API |

For OpenAI-compatible providers, pick a preset to fill in the server URL, or
type your own, and set the model id.

| Preset | Server URL | Example model |
|---|---|---|
| OpenAI | `https://api.openai.com/v1` | `gpt-4o-mini` |
| OpenRouter | `https://openrouter.ai/api/v1` | `openrouter/auto` |
| Gemini | `https://generativelanguage.googleapis.com/v1beta/openai` | `gemini-2.0-flash` |
| Ollama (local) | `http://localhost:11434/v1` | `llama3.2-vision` |

The model must accept images to answer about a selection.

## Your key

Paste the key in **Settings → AI**. It is stored in your operating system's
keyring (Windows Credential Manager; Secret Service, such as GNOME Keyring or
KWallet, on Linux). It is never written to the config file, logs or crash
output. If no keyring is available OpenCircle says so rather than saving the
key in a file. The key is sent only to the server you configured.

Plain `http://` is accepted only for your own machine (`localhost`); a remote
server with a key must use `https://`.

## Local models (no key, nothing leaves your PC)

1. Install [Ollama](https://ollama.com) or LM Studio and download a vision
   model (for example `llama3.2-vision`).
2. In Settings → AI choose **OpenAI-compatible**, preset **Ollama (local)**
   (or LM Studio's `http://localhost:1234/v1`), and the model name.
3. Leave the key empty. Selections go to your own machine only.

## Using it

Circle something and press `A` (or use the **Explain with AI** quick action,
`Ctrl+Shift+E`). The answer streams in a side panel, and you can ask follow-up
questions. Only the selected area and your questions are sent, with the
conversation so far. Closing the panel stops a running answer.

## Config

```toml
[ai]
provider = "anthropic"            # anthropic | openai_compatible
base_url = ""                     # "" = provider default; required for openai_compatible
model = "claude-sonnet-5-5"
```

The key is not in this file; set it in Settings.
