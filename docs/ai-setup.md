# AI setup

OpenCircle has no AI of its own and no account. By default, Ask AI and Explain
open your question in Google AI Mode in your own browser, with no key. If you want answers from an AI provider (with follow-up questions) and AI
translation, choose a provider with your own key. Nothing is sent until you ask
a question.

## Browser (no key): the default

Nothing to set up. In **Settings → Ask AI**, **Browser** is the default. When you
press `Enter` in the bar's AI field (or use the **Explain with AI** quick
action), OpenCircle opens your question in your default browser. Nothing opens
in your browser by itself: only asking does it.

- With **Include screen** on (the default) and something selected, the circled
  picture goes with the question: OpenCircle opens a Google Lens page in your
  browser, and your browser sends the picture and the question to Google. The
  answer appears there (an AI overview about the picture; its **AI Mode** tab
  continues the conversation). No text reader or download is involved.
- With **Include screen** off, or nothing selected, only the question is sent: it
  opens in Google AI Mode.
- Long questions are shortened at a word boundary (about 1,800 characters) and
  OpenCircle says so.
- **Translate in place needs a provider with a key** (below): the browser cannot
  send text back.
- The picture goes from your browser straight to Google, only when you press
  asking; OpenCircle uploads it nowhere else. See [Privacy](privacy.md).

## Providers with your own key

In **Settings → Ask AI**, choose **AI provider**, then pick your provider from the
**Provider** list. OpenCircle picks the right way to talk to each one and fills
in its server; you pick a model (the default below is filled in) and paste your key.

| Provider | Server | Default model |
|---|---|---|
| OpenRouter | `https://openrouter.ai/api/v1` | `openrouter/free` (OpenRouter picks a free model) |
| OpenAI | `https://api.openai.com/v1` | `gpt-4o-mini` |
| Anthropic | Anthropic's own | `claude-sonnet-5-5` |
| Google Gemini | `https://generativelanguage.googleapis.com/v1beta/openai` | `gemini-flash-lite-latest` |
| Groq | `https://api.groq.com/openai/v1` | `qwen/qwen3.8-27b` |
| Mistral | `https://api.mistral.ai/v1` | `mistral-small-latest` |
| DeepSeek | `https://api.deepseek.com/v1` | `deepseek-flash` |
| xAI | `https://api.x.ai/v1` | `grok-4.3` |
| Ollama (local) | `http://localhost:11434/v1` | `llama3.2-vision` |
| LM Studio (local) | `http://localhost:1234/v1` | `local-model` |
| Custom server | the URL you type | yours |

**Custom server** is any server that speaks the OpenAI chat-completions API;
it is the only choice that asks for a URL.

The model must accept images to answer about a selection. A model you chose
yourself stays when OpenCircle's default changes.

### The model list

**Model** is a list with a search: each provider's current chat models, built
in, so it shows offline. Where the provider's own pricing page or list says so,
a model carries its tag ("Free tier at Google", "Paid at OpenAI"); OpenCircle
itself charges nothing. **Load models from** (your provider) asks the provider for
its full list, with your key, only when you click it. You can also type any
model id.

### Errors

When the provider refuses or fails, the chat shows a calm card: a short line
saying what happened, the provider's own message (two lines; **Show details**
for the rest) and one fix: **Choose another model** (opens the list), **Check
the key**, **Open settings** or **Retry**. A busy server (503 or 529) is retried
once by itself after two seconds, and a connection that drops before any answer
is tried again up to twice on a new connection (then the card says "The
connection dropped"). Every provider error goes into the local log
without your key or your question, so **Copy diagnostics** can include it.

## Your key

Paste the key in **Settings → Ask AI**. Each provider keeps its own key: a key
saved for one provider is never sent to another (or to a Custom server's
URL); after switching provider, paste that provider's key. It is stored in
your operating system's
keyring (Windows Credential Manager; Secret Service, such as GNOME Keyring or
KWallet, on Linux). It is never written to the config file, logs or crash
output. If no keyring is available OpenCircle says so rather than saving the
key in a file. The key is sent only to the server you configured.

Plain `http://` is accepted only for your own machine (`localhost`); a remote
server with a key must use `https://`.

## Local models (no key, nothing leaves your PC)

1. Install [Ollama](https://ollama.com) or LM Studio and download a vision
   model (for example `llama3.2-vision`).
2. In Settings → Ask AI choose **AI provider**, then **Ollama (local)** (or **LM
   Studio (local)**, `http://localhost:1234/v1`) from the Provider list, and set
   the model name.
3. Leave the key empty. Selections go to your own machine only.

## Using it

Circle something and type in the bar's AI field, or press `A` (or use the
**Explain with AI** quick action, `Ctrl+Shift+E`). With a provider and key, the
answer streams into a chat with markdown, and you can ask follow-up questions: a
follow-up pins the chat: it becomes its own window, the overlay closes by itself,
the chat stays open above other apps and a new capture asks into the same chat
(**Settings → Ask AI → AI chat stays open** changes this). While the chat is open
on the overlay, the bar stays beside it. Snip's **More → Ask AI** sends the picked
area with your marks on it. Only
the selected area (while **Include screen** is on) and your questions are sent,
with the last 20 messages and one short summary of older ones. Chats are saved on
this PC as text only (**Keep chat history** turns that off). With **Browser**,
asking opens Google in your browser instead, with the picture (see above).

## Config

```toml
[ai]
provider = "browser"              # browser | anthropic | openai_compatible (Settings shows service names)
base_url = ""                     # "" = provider default; required for openai_compatible
model = "claude-sonnet-5-5"
```

`browser` is the default when the file has no `provider`. Settings made before
it existed (`anthropic` or `openai_compatible`) keep working unchanged. The key
is not in this file; set it in Settings.
