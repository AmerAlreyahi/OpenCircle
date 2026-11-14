# Notices

OpenCircle is MIT-licensed (see [LICENSE](../LICENSE.md)). Apart from a small built-in
language detector (below), it ships no third-party models; the files below
are downloaded on first use, after you agree, and are covered by their own
licences.

## Offline translation

OPUS-MT models by Jörg Tiedemann and the Language Technology Research Group
at the University of Helsinki (Helsinki-NLP), one per direction. Paper:
Tiedemann and Thottingal, "OPUS-MT - Building open translation services for
the World", EAMT 2020. ONNX export and int8 quantization: Xenova
(Transformers.js), `https://huggingface.co/Xenova/opus-mt-<pair>` at the
commit below. OpenCircle uses the encoders and tokenizer files unmodified.
Every decoder (these and the five below) is changed by OpenCircle in one
way: the output layer, built in both branches of the merged decoder, is
built once before them (scripts/lighten-mt-decoders.py; same weights, same
translations, less memory); the changed decoders are downloaded from the
OpenCircle-models release (models-v2). These lighter models are published as
OptimaMT (https://huggingface.co/OptimaMT, https://github.com/AmerAlreyahi/OptimaMT),
under each pair's original licence below.

| Pair | Licence | Model | Xenova commit |
|---|---|---|---|
| en-ar | CC BY 4.0 | [Helsinki-NLP/opus-mt-en-ar](https://huggingface.co/Helsinki-NLP/opus-mt-en-ar) | 034a684356c1 |
| en-fr | Apache-2.0 | [Helsinki-NLP/opus-mt-en-fr](https://huggingface.co/Helsinki-NLP/opus-mt-en-fr) | 28726206f808 |
| en-es | Apache-2.0 | [Helsinki-NLP/opus-mt-en-es](https://huggingface.co/Helsinki-NLP/opus-mt-en-es) | 4b002a4c7edd |
| en-de | CC BY 4.0 | [Helsinki-NLP/opus-mt-en-de](https://huggingface.co/Helsinki-NLP/opus-mt-en-de) | 1ca130c44c4c |
| en-ru | Apache-2.0 | [Helsinki-NLP/opus-mt-en-ru](https://huggingface.co/Helsinki-NLP/opus-mt-en-ru) | e050376e9601 |
| en-zh | Apache-2.0 | [Helsinki-NLP/opus-mt-en-zh](https://huggingface.co/Helsinki-NLP/opus-mt-en-zh) | 046f55aec303 |
| ar-en | Apache-2.0 | [Helsinki-NLP/opus-mt-ar-en](https://huggingface.co/Helsinki-NLP/opus-mt-ar-en) | 09c755686640 |
| fr-en | Apache-2.0 | [Helsinki-NLP/opus-mt-fr-en](https://huggingface.co/Helsinki-NLP/opus-mt-fr-en) | 6b166a182780 |
| zh-en | CC BY 4.0 | [Helsinki-NLP/opus-mt-zh-en](https://huggingface.co/Helsinki-NLP/opus-mt-zh-en) | 39d480d52a9e |
| ja-en | Apache-2.0 | [Helsinki-NLP/opus-mt-ja-en](https://huggingface.co/Helsinki-NLP/opus-mt-ja-en) | 1a906cfaaf7c |
| es-en | Apache-2.0 | [Helsinki-NLP/opus-mt-es-en](https://huggingface.co/Helsinki-NLP/opus-mt-es-en) | eadfd7c658a9 |
| de-en | Apache-2.0 | [Helsinki-NLP/opus-mt-de-en](https://huggingface.co/Helsinki-NLP/opus-mt-de-en) | 399dfd687067 |
| ru-en | CC BY 4.0 | [Helsinki-NLP/opus-mt-ru-en](https://huggingface.co/Helsinki-NLP/opus-mt-ru-en) | afe8c6c738ec |
| it-en | Apache-2.0 | [Helsinki-NLP/opus-mt-it-en](https://huggingface.co/Helsinki-NLP/opus-mt-it-en) | fd0b89b9c052 |
| nl-en | Apache-2.0 | [Helsinki-NLP/opus-mt-nl-en](https://huggingface.co/Helsinki-NLP/opus-mt-nl-en) | 82c42d95d150 |
| uk-en | Apache-2.0 | [Helsinki-NLP/opus-mt-uk-en](https://huggingface.co/Helsinki-NLP/opus-mt-uk-en) | b2ad6afa424a |
| pl-en | Apache-2.0 | [Helsinki-NLP/opus-mt-pl-en](https://huggingface.co/Helsinki-NLP/opus-mt-pl-en) | aeaf0e003b04 |
| hi-en | Apache-2.0 | [Helsinki-NLP/opus-mt-hi-en](https://huggingface.co/Helsinki-NLP/opus-mt-hi-en) | 99853100ed9d |
| id-en | Apache-2.0 | [Helsinki-NLP/opus-mt-id-en](https://huggingface.co/Helsinki-NLP/opus-mt-id-en) | c38ef36c843b |
| vi-en | Apache-2.0 | [Helsinki-NLP/opus-mt-vi-en](https://huggingface.co/Helsinki-NLP/opus-mt-vi-en) | 541dfd72ec07 |
| th-en | Apache-2.0 | [Helsinki-NLP/opus-mt-th-en](https://huggingface.co/Helsinki-NLP/opus-mt-th-en) | 7f0406b972f7 |
| ko-en | Apache-2.0 | [Helsinki-NLP/opus-mt-ko-en](https://huggingface.co/Helsinki-NLP/opus-mt-ko-en) | d9fa1ac60082 |
| en-it | Apache-2.0 | [Helsinki-NLP/opus-mt-en-it](https://huggingface.co/Helsinki-NLP/opus-mt-en-it) | 075406e3c8c2 |
| en-nl | Apache-2.0 | [Helsinki-NLP/opus-mt-en-nl](https://huggingface.co/Helsinki-NLP/opus-mt-en-nl) | c3a0541231a7 |
| en-uk | Apache-2.0 | [Helsinki-NLP/opus-mt-en-uk](https://huggingface.co/Helsinki-NLP/opus-mt-en-uk) | e6ca58729298 |
| en-hi | Apache-2.0 | [Helsinki-NLP/opus-mt-en-hi](https://huggingface.co/Helsinki-NLP/opus-mt-en-hi) | 7aad72006a15 |
| en-id | Apache-2.0 | [Helsinki-NLP/opus-mt-en-id](https://huggingface.co/Helsinki-NLP/opus-mt-en-id) | b3b41f654c2f |
| en-vi | Apache-2.0 | [Helsinki-NLP/opus-mt-en-vi](https://huggingface.co/Helsinki-NLP/opus-mt-en-vi) | 30bcd461c0d7 |
| de-es | Apache-2.0 | [Helsinki-NLP/opus-mt-de-es](https://huggingface.co/Helsinki-NLP/opus-mt-de-es) | d339419ee3bd |
| de-fr | Apache-2.0 | [Helsinki-NLP/opus-mt-de-fr](https://huggingface.co/Helsinki-NLP/opus-mt-de-fr) | e783da03d247 |
| es-de | Apache-2.0 | [Helsinki-NLP/opus-mt-es-de](https://huggingface.co/Helsinki-NLP/opus-mt-es-de) | 7f493e4b81b6 |
| es-fr | Apache-2.0 | [Helsinki-NLP/opus-mt-es-fr](https://huggingface.co/Helsinki-NLP/opus-mt-es-fr) | 3bef5f612825 |
| es-it | Apache-2.0 | [Helsinki-NLP/opus-mt-es-it](https://huggingface.co/Helsinki-NLP/opus-mt-es-it) | 596c67cda7cb |
| es-ru | Apache-2.0 | [Helsinki-NLP/opus-mt-es-ru](https://huggingface.co/Helsinki-NLP/opus-mt-es-ru) | 268ccd60bde3 |
| fr-de | Apache-2.0 | [Helsinki-NLP/opus-mt-fr-de](https://huggingface.co/Helsinki-NLP/opus-mt-fr-de) | 7eea1996b448 |
| fr-es | Apache-2.0 | [Helsinki-NLP/opus-mt-fr-es](https://huggingface.co/Helsinki-NLP/opus-mt-fr-es) | 962338be5103 |
| fr-ru | Apache-2.0 | [Helsinki-NLP/opus-mt-fr-ru](https://huggingface.co/Helsinki-NLP/opus-mt-fr-ru) | c2ec732b045a |
| it-es | Apache-2.0 | [Helsinki-NLP/opus-mt-it-es](https://huggingface.co/Helsinki-NLP/opus-mt-it-es) | 0eba27fe6da2 |
| it-fr | Apache-2.0 | [Helsinki-NLP/opus-mt-it-fr](https://huggingface.co/Helsinki-NLP/opus-mt-it-fr) | 7843e3b7137d |
| ru-es | Apache-2.0 | [Helsinki-NLP/opus-mt-ru-es](https://huggingface.co/Helsinki-NLP/opus-mt-ru-es) | 9cb5fb9fae4d |
| ru-fr | Apache-2.0 | [Helsinki-NLP/opus-mt-ru-fr](https://huggingface.co/Helsinki-NLP/opus-mt-ru-fr) | c7275f468246 |
| ru-uk | Apache-2.0 | [Helsinki-NLP/opus-mt-ru-uk](https://huggingface.co/Helsinki-NLP/opus-mt-ru-uk) | 6d8a744ed6ea |
| uk-ru | Apache-2.0 | [Helsinki-NLP/opus-mt-uk-ru](https://huggingface.co/Helsinki-NLP/opus-mt-uk-ru) | cfb76ccd2077 |
| nl-fr | Apache-2.0 | [Helsinki-NLP/opus-mt-nl-fr](https://huggingface.co/Helsinki-NLP/opus-mt-nl-fr) | 6c8ebb68522f |

en-ar is the Tatoeba-Challenge eng-ara model, released under CC BY 4.0
(https://creativecommons.org/licenses/by/4.0/; its Hugging Face card lists
Apache-2.0); original weights:
https://github.com/Helsinki-NLP/Tatoeba-Challenge/tree/master/models/eng-ara.
The others: the licence their Hugging Face model card lists (Apache-2.0,
https://www.apache.org/licenses/LICENSE-2.0; CC BY 4.0,
https://creativecommons.org/licenses/by/4.0/).

Five direct Arabic pairs Xenova never exported were converted by OpenCircle
from Helsinki-NLP's own weights the same way (optimum ONNX export, merged
decoder, int8 dynamic quantization; scripts/convert-mt-models.py), weights
unchanged, and are downloaded from the OpenCircle-models release (models-v1).
OPUS-MT / Tatoeba-Challenge models, CC BY 4.0
(https://creativecommons.org/licenses/by/4.0/; their Hugging Face cards list
Apache-2.0):

| Pair | Model | Helsinki-NLP commit |
|---|---|---|
| fr-ar | [Helsinki-NLP/opus-mt-fr-ar](https://huggingface.co/Helsinki-NLP/opus-mt-fr-ar) | 5469dc759cd9 |
| es-ar | [Helsinki-NLP/opus-mt-es-ar](https://huggingface.co/Helsinki-NLP/opus-mt-es-ar) | 76efb7da47ad |
| ar-fr | [Helsinki-NLP/opus-mt-ar-fr](https://huggingface.co/Helsinki-NLP/opus-mt-ar-fr) | e507d7bc8508 |
| ar-es | [Helsinki-NLP/opus-mt-ar-es](https://huggingface.co/Helsinki-NLP/opus-mt-ar-es) | da86e973e723 |
| ar-de | [Helsinki-NLP/opus-mt-ar-de](https://huggingface.co/Helsinki-NLP/opus-mt-ar-de) | 8e6cbcce68fe |

Portuguese (en-pt, pt-en) uses Helsinki-NLP's multi-language Romance models
(Apache-2.0, https://www.apache.org/licenses/LICENSE-2.0), converted by
OpenCircle the same way (weights unchanged, decoder lightened as above) and
downloaded from the OpenCircle-models release (models-v3). en-pt asks the
model for Brazilian Portuguese (`>>pt_br<<`).

| Pair | Model | Helsinki-NLP commit |
|---|---|---|
| en-pt | [Helsinki-NLP/opus-mt-en-ROMANCE](https://huggingface.co/Helsinki-NLP/opus-mt-en-ROMANCE) | f8f3a28e8b62 |
| pt-en | [Helsinki-NLP/opus-mt-ROMANCE-en](https://huggingface.co/Helsinki-NLP/opus-mt-ROMANCE-en) | e9ca9975e397 |

## Built into the app

Short-line language detection: whichlang by Quickwit (MIT),
https://github.com/quickwit-oss/whichlang, built into the app.

Preview video decoder: rav1d (BSD-2-Clause), https://github.com/memorysafety/rav1d,
built into the app. Copyright (c) 2018-2019, VideoLAN and dav1d authors;
Copyright (c) 2023-2024, VideoLAN, dav1d authors, and Internet Security Research
Group. All rights reserved. Redistribution and use in source and binary forms,
with or without modification, are permitted provided that the following
conditions are met: 1. Redistributions of source code must retain the above
copyright notice, this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice,
this list of conditions and the following disclaimer in the documentation
and/or other materials provided with the distribution. THIS SOFTWARE IS
PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT
SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE
OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

## Text recognition (OCR)

PaddleOCR PP-OCRv5 models (Apache-2.0): the text detector and one
recognizer per script (English, Latin, Arabic, Cyrillic, Chinese/Japanese),
ONNX export by oar-ocr (https://github.com/GreatV/oar-ocr, Apache-2.0),
release v0.3.0.
The app runs them with its own pre- and post-processing, which follows
PaddleOCR's and oar-ocr's. Box growing: Clipper2 by Angus Johnson, Rust
port clipper2-rust (BSL-1.0). Border following: Suzuki and Abe's
algorithm as imageproc (MIT, https://github.com/image-rs/imageproc) runs
it.

## ONNX Runtime

ONNX Runtime by Microsoft (MIT), https://github.com/microsoft/onnxruntime.

## Bundled fonts

Inter by Rasmus Andersson, Noto Sans Arabic by Google and JetBrains Mono by
JetBrains, all under the SIL Open Font License 1.1 (assets/fonts/OFL-Inter.txt,
OFL-NotoSansArabic.txt, OFL-JetBrainsMono.txt), subset. Lucide icons (ISC,
assets/lucide/LICENSE), subset.

## Renderer

iced_tiny_skia (MIT, Copyright 2019 Hector Ramon, Iced contributors) is built
from vendor/iced_tiny_skia with one small fix; its licence is beside it.

## Preview clips

The short recordings at the top of Settings pages (a still of each bundled in
the app, assets/previews; the full clips downloaded when a preview is played)
are the owner's approved edits. They show third-party material under
its own terms; the app's MIT licence does not relicense it. CC0 credits are
courtesy credits. OpenCircle's code remains MIT; the media exceptions below
(Unsplash License for the COFFEE photo, CC BY-SA 4.0 for the web-page
translation preview) apply only to the named clips. The credits identify sources; they do not clear unrelated
search-result thumbnails, third-party interfaces or data-provider terms shown
in the clips.

Each clip has its own section below (Settings > About > Preview clip credits
links to it).

### Succulent photograph (circle-1)

José Gabriel Jara Porras, WordPress Photo Directory, CC0 1.0. Cropped and
shown in an edited OpenCircle screen recording.
https://wordpress.org/photos/photo/4569e13887/
https://creativecommons.org/publicdomain/zero/1.0/

### Lotus photograph (circle-2)

Shubham Patil, WordPress Photo Directory, CC0 1.0. Cropped and shown in an
edited OpenCircle screen recording.
https://wordpress.org/photos/photo/4766ac73e9/
https://creativecommons.org/publicdomain/zero/1.0/

### Kingfisher photograph (circle-3)

sebascordero15, WordPress Photo Directory, CC0 1.0. Cropped and shown in an
edited OpenCircle screen recording.
https://wordpress.org/photos/photo/11669e53a4/
https://creativecommons.org/publicdomain/zero/1.0/

### COFFEE photograph (text)

Tim Mossholder on Unsplash, photo jozE5L60Aj4, used under the Unsplash
License. Shown in an edited OpenCircle screen recording. No endorsement
implied. MEDIA EXCEPTION: the photo in this clip stays under the Unsplash
License (not MIT, not CC0).
https://unsplash.com/photos/a-sign-on-a-brick-wall-that-says-coffee-jozE5L60Aj4
https://unsplash.com/license

### Wikipedia translation (translate-page)

Based on "Plante d'intérieur" by Wikipedia contributors, CC BY-SA 4.0.
Machine translation into English, German, Arabic and Spanish shown by
OpenCircle; framing, timing edits, captions and active-state graphics added
for the preview. MEDIA EXCEPTION: this preview clip (translate-page.ivf) and
its still (assets/previews/stills/translate-page.webp) are licensed under CC
BY-SA 4.0, including the adapter's contributions; this does not change the
app code's MIT licence. Underlying public-domain material remains public
domain; third-party names and trademarks keep their separate rights. The
exact recorded Wikipedia revision has not been established.

- Article: https://fr.wikipedia.org/wiki/Plante_d%27int%C3%A9rieur
- History: https://fr.wikipedia.org/w/index.php?title=Plante_d%27int%C3%A9rieur&action=history
- Licence: https://creativecommons.org/licenses/by-sa/4.0/ (legal code:
  https://github.com/AmerAlreyahi/OpenCircle/blob/main/LICENSES/CC-BY-SA-4.0.txt)
- Embedded photograph "Her, IFA 2018, Berlin (P1070094).jpg" (c) Matti Blume,
  CC BY-SA 4.0:
  https://fr.wikipedia.org/wiki/Fichier:Her,_IFA_2018,_Berlin_(P1070094).jpg
- Embedded painting by Carl Larsson, "Flowers on the Windowsill", public
  domain:
  https://commons.wikimedia.org/wiki/File:Blomsterf%C3%B6nstret_av_Carl_Larsson_1894.jpg

### Game screens (translate-2, translate-3)

Original images made for OpenCircle's previews (no real game).

### Solar photovoltaic panel prices (ai-chat, ai-browser)

The same chart in both clips: Our World in Data; IRENA (2025) and other
sources, processed by Our World in Data, as credited in the recording. OWID
visualization: CC BY 4.0. Cropped and time-edited screen recordings with
OpenCircle active-state graphics.
https://ourworldindata.org/grapher/solar-pv-prices
https://creativecommons.org/licenses/by/4.0/

### Tourism (snip-long)

Bastian Herre and Veronika Samborska (2023), "Tourism", Our World in Data, CC
BY 4.0 for OWID-produced article and visualizations. Cropped, time-edited
screen recording. The chart "Arrivals of tourists from abroad" credits UN
Tourism (2025); other chart and provider notices in the recording are kept.
https://ourworldindata.org/tourism
https://ourworldindata.org/grapher/international-tourist-trips
https://creativecommons.org/licenses/by/4.0/
https://ourworldindata.org/faqs

### Not affiliated with Google

OpenCircle is an independent project and is not affiliated with, sponsored
by, or endorsed by Google. Google and its product names and logos are
trademarks of their respective owners. Depicted third-party services and
content are shown for demonstration only.
