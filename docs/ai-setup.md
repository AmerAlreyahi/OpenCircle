# AI setup

OpenCircle has no AI of its own and no account. By default, Ask AI and Explain
open your question in Google AI Mode in your own browser, with no key. If you want answers from an AI provider (with follow-up questions) and AI
translation, choose a provider with your own key. Nothing is sent until you ask
a question.

## Google AI Mode (no key) — the default

Nothing to set up. When you press Send in the Ask AI panel (or Explain on a
word card, or the **Explain with AI** quick action), OpenCircle shows Google
Search's AI Mode with your question already typed in, in your default browser.
Nothing opens in your browser by itself: only Send does it.

- The question is your prompt plus the text read from your selection. Reading
  the text uses the same offline text reader as **Select text**; the first time,
  OpenCircle asks before downloading it (about 30 MB). If you say no, only
  your prompt is sent, and the panel tells you the image could not be passed.
- The answer appears in your browser, where you can ask follow-ups on Google's
  page. Press **Search the image with Lens** for questions about the picture
  itself.
- Long questions are shortened at a word boundary (about 1,800 characters) and
  the panel says so.
- **Translate in place needs a provider with a key** (below): the browser cannot
  send text back.
- Only the question text goes to Google, from your browser, when you press
  Send. The screenshot is not uploaded unless you choose Lens. See
  [Privacy](privacy.md).

## Providers with your own key

Two kinds, set in **Settings → AI** (choose **Anthropic** or **OpenAI-like**):

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
`Ctrl+Shift+E`). With a key, the answer streams in a side panel and you can ask
follow-up questions. Only the selected area and your questions are sent, with
the conversation so far. Closing the panel stops a running answer. With the
browser provider, Send opens Google AI Mode in your browser instead (see above).

## Config

```toml
[ai]
provider = "browser"              # browser | anthropic | openai_compatible
base_url = ""                     # "" = provider default; required for openai_compatible
model = "claude-sonnet-5-5"
```

`browser` is the default when the file has no `provider`. Settings made before
it existed (`anthropic` or `openai_compatible`) keep working unchanged. The key
is not in this file; set it in Settings.
