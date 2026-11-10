# Security policy

## Report a problem privately

Please do not open a public issue for a security problem. Use GitHub's private
reporting instead: open the [Security tab](https://github.com/AmerAlreyahi/OpenCircle/security)
and press **Report a vulnerability**.

Please include:

- what you found and why it matters;
- the OpenCircle version, your OS and version (and desktop, Wayland or X11, on Linux);
- steps to reproduce it, or a small proof of concept;
- anything you already tried.

Please do not include real passwords, API keys or private screenshots.

## What to expect

OpenCircle is a one-person project. I reply within 7 days, on a best-effort
basis, and I will tell you when it is fixed. There is no bug bounty.

## Supported versions

Only the latest release gets security fixes. Please check that the problem
still exists in the latest release before you report it.

## In scope

- the app itself (capture, text reading, translate, AI, snip);
- the updater and its signature and checksum checks;
- model and preview downloads (their checksums);
- the one-time local page that is handed to your browser for Search and Ask AI;
- anything that sends a capture or a key somewhere the user did not choose.

## Out of scope

- problems in Google Lens, your AI provider, Windows or your desktop;
- attacks that need an already compromised computer or administrator rights;
- reports from automated scanners without a working example.
