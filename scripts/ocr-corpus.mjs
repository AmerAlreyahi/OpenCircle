// Renders the screen-text OCR corpus (#169): our own encyclopedia-style pages
// (Arabic, English, mixed) as screenshots, each with its ground truth: every
// rendered line's text (logical order) and pixel box, read from the DOM.
// Test data only, never shipped. Needs playwright-core and its Chromium:
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core \
//   WINFONTS=/mnt/c/Windows/Fonts EXTRA_FONTS=/path/to/libre-fonts \
//   node scripts/ocr-corpus.mjs <out dir>
// Writes <out>/<page>.png + <page>.json and <out>/index.json. The fonts are
// linked into <out>/fonts (never copied) and fontconfig sees only them, so
// the pages render as on Windows, not with the machine's own fonts.
import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';

const out = path.resolve(process.argv[2] || 'target/ocr-corpus');
const { chromium } = createRequire(import.meta.url)(process.env.PLAYWRIGHT_CORE || 'playwright-core');
const winFonts = process.env.WINFONTS || '/mnt/c/Windows/Fonts';
const extraFonts = process.env.EXTRA_FONTS || '';

// family -> files (Windows fonts by file name; libre ones from EXTRA_FONTS)
const FONT_FILES = [
  'segoeui.ttf', 'segoeuib.ttf', 'arial.ttf', 'arialbd.ttf', 'tahoma.ttf', 'tahomabd.ttf',
  'times.ttf', 'timesbd.ttf', 'trado.ttf', 'tradbdo.ttf', 'arabtype.ttf', 'majalla.ttf',
  'majallab.ttf', 'simpo.ttf', 'simpbdo.ttf', 'aldhabi.ttf', 'andlso.ttf', 'calibri.ttf',
  'verdana.ttf', 'georgia.ttf', 'consola.ttf', 'seguisym.ttf',
];

function setupFonts() {
  const dir = path.join(out, 'fonts');
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  for (const f of FONT_FILES) {
    const src = path.join(winFonts, f);
    if (fs.existsSync(src)) fs.symlinkSync(src, path.join(dir, f));
  }
  if (extraFonts) {
    for (const f of fs.readdirSync(extraFonts)) {
      if (/\.(ttf|otf)$/i.test(f)) fs.symlinkSync(path.join(extraFonts, f), path.join(dir, f));
    }
  }
  const conf = path.join(out, 'fonts.conf');
  fs.writeFileSync(conf, `<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig>
  <dir>${dir}</dir>
  <cachedir>${path.join(out, 'fc-cache')}</cachedir>
  <alias><family>sans-serif</family><prefer><family>Segoe UI</family><family>Arial</family></prefer></alias>
  <alias><family>serif</family><prefer><family>Times New Roman</family></prefer></alias>
  <alias><family>monospace</family><prefer><family>Consolas</family></prefer></alias>
  <match target="font"><edit name="antialias" mode="assign"><bool>true</bool></edit>
    <edit name="hinting" mode="assign"><bool>true</bool></edit>
    <edit name="hintstyle" mode="assign"><const>hintslight</const></edit>
    <edit name="rgba" mode="assign"><const>none</const></edit></match>
</fontconfig>`);
  return conf;
}

// ---- Text (written for this test) -------------------------------------------------

const AR = {
  site: 'الموسوعة الحرة',
  nav: ['الصفحة الرئيسية', 'أحدث التغييرات', 'مقالة عشوائية', 'تصفح المواضيع', 'المساعدة', 'الميدان', 'تبرع للموسوعة', 'اتصل بنا'],
  tools: ['أدوات', 'ماذا يصل هنا', 'تغييرات ذات علاقة', 'رفع ملف', 'صفحات خاصة', 'وصلة دائمة', 'معلومات الصفحة', 'استشهد بهذه الصفحة'],
  title: 'القهوة',
  tagline: 'من الموسوعة الحرة، المعرفة للجميع',
  tabs: ['مقالة', 'نقاش', 'اقرأ', 'عدّل', 'تاريخ'],
  lead: [
    'القهوة مشروب يُعدّ من بذور شجيرة البن بعد تحميصها وطحنها ونقعها في الماء الساخن، وهي من أكثر المشروبات انتشارًا في العالم. يتميز طعمها بالمرارة الخفيفة وبرائحة قوية تختلف باختلاف درجة التحميص وطريقة التحضير.',
    'تحتوي القهوة على مادة الكافيين المنبهة التي تزيد اليقظة وتقلل الشعور بالتعب لدى كثير من الناس، ولذلك ارتبط شربها بساعات الصباح وبمجالس العمل والدراسة. وتختلف عادات تقديمها من بلد إلى آخر، فبعض الشعوب تضيف إليها الهيل أو الحليب أو السكر، وبعضها يفضلها سادة.',
  ],
  sections: [
    ['التاريخ', [
      'تشير الروايات المتداولة إلى أن القهوة عُرفت أولًا في المرتفعات الواقعة جنوب البحر الأحمر، ثم انتقلت زراعتها إلى اليمن حيث صارت تجارة مهمة عبر موانئه. ومن هناك انتشر شربها في مدن الحجاز والشام ومصر، ثم وصلت إلى إسطنبول وأوروبا عبر طرق التجارة البحرية.',
      'في القرون التالية أصبحت المقاهي أماكن للقاء والحديث وقراءة الأخبار، وكان روادها من التجار والكتاب والطلاب. وقد أثار انتشارها في بعض المدن جدلًا بين مؤيد ومعارض، لكنها بقيت جزءًا من الحياة اليومية حتى اليوم.',
    ]],
    ['الزراعة', [
      'تنمو شجيرة البن في المناطق الاستوائية ذات الأمطار المنتظمة والتربة الخصبة، وتحتاج إلى ظل جزئي ودرجات حرارة معتدلة. وتنتج الشجيرة ثمارًا حمراء صغيرة تحوي كل ثمرة منها بذرتين في الغالب، وتُقطف الثمار يدويًا أو آليًا بحسب طبيعة الأرض.',
      'بعد القطف تُفصل البذور عن اللب بطرق مختلفة، منها الطريقة الجافة التي تُترك فيها الثمار في الشمس، والطريقة الرطبة التي تُغسل فيها بالماء. ثم تُجفف البذور وتُفرز حسب الحجم والجودة قبل أن تُعبأ للتصدير.',
    ]],
    ['التحميص والتحضير', [
      'يغير التحميص لون البذور من الأخضر إلى البني ويطلق الزيوت العطرية فيها. فالتحميص الخفيف يحفظ الحموضة والنكهات الزهرية، أما التحميص الداكن فيمنح طعمًا أثقل ومرارة أوضح.',
      'من طرق التحضير الشائعة الغلي في الدلة أو الركوة، والتقطير بالورق، والضغط بالمكبس، وآلات الإسبريسو التي تدفع الماء الساخن تحت ضغط عالٍ خلال البن المطحون ناعمًا.',
    ]],
  ],
  infobox: {
    caption: 'فنجان قهوة مع حبوب البن المحمصة',
    rows: [['النوع', 'مشروب ساخن'], ['المكون الرئيسي', 'بذور البن'], ['بلد المنشأ', 'جنوب البحر الأحمر'], ['درجة التقديم', 'ساخنة أو باردة'], ['المادة الفعالة', 'الكافيين']],
  },
  toc: ['المحتويات', '١ التاريخ', '٢ الزراعة', '٣ التحميص والتحضير', '٤ انظر أيضًا'],
  table: {
    caption: 'إنتاج تقديري لبعض البلدان (بالألف طن)',
    head: ['البلد', 'الإنتاج', 'النوع الغالب'],
    rows: [['البلد الأول', '٣٢٠٠', 'أرابيكا'], ['البلد الثاني', '١٨٥٠', 'روبوستا'], ['البلد الثالث', '٨٧٠', 'أرابيكا'], ['البلد الرابع', '٤٦٠', 'مختلط']],
  },
  see: ['انظر أيضًا', 'الشاي', 'الكاكاو', 'المقهى', 'الهيل'],
  footer: 'عُدلت هذه الصفحة آخر مرة في ١٢ مارس ٢٠٢٦. النصوص متاحة بموجب رخصة حرة، وقد تُطبق شروط إضافية.',
};

const EN = {
  site: 'The Free Encyclopedia',
  nav: ['Main page', 'Recent changes', 'Random article', 'Browse topics', 'Help', 'Community portal', 'Donate', 'Contact us'],
  tools: ['Tools', 'What links here', 'Related changes', 'Upload file', 'Special pages', 'Permanent link', 'Page information', 'Cite this page'],
  title: 'Lighthouse',
  tagline: 'From the free encyclopedia, knowledge for everyone',
  tabs: ['Article', 'Talk', 'Read', 'Edit', 'History'],
  lead: [
    'A lighthouse is a tower or other structure that sends out light from a system of lamps and lenses to guide ships at sea or on inland waters. Lighthouses mark dangerous coastlines, hazardous shoals, reefs and safe entries to harbours.',
    'Their number has fallen as satellite navigation became common, but many still work today, often automated and watched from a distance. Some of the old towers are now museums, hotels or simple landmarks on the coast.',
  ],
  sections: [
    ['History', [
      'Before towers were built, sailors relied on fires lit on hilltops near the shore. Later, stone towers raised the flame higher so that it could be seen from farther away, and keepers lived beside them to tend the light through the night.',
      'In the nineteenth century new lenses made of many glass rings bent the light into a strong beam, and clockwork turned the lens so each station flashed in its own rhythm that ships could recognise.',
    ]],
    ['Design', [
      'A typical tower has a lantern room at the top, a gallery around it, and a stair that winds up through the shaft. Thick walls protect the structure from wind and waves, and bands of paint make it visible by day.',
    ]],
  ],
  infobox: {
    caption: 'A stone lighthouse on a rocky cape',
    rows: [['Type', 'Navigational aid'], ['Material', 'Stone, brick or steel'], ['Height', '10 to 100 metres'], ['Range', 'Up to 40 km'], ['Power', 'Electric or solar']],
  },
  toc: ['Contents', '1 History', '2 Design', '3 See also'],
  table: {
    caption: 'Example light characters',
    head: ['Pattern', 'Period', 'Meaning'],
    rows: [['Fixed', 'steady', 'Harbour entrance'], ['Flashing', '5 s', 'Coastal light'], ['Occulting', '10 s', 'Channel mark'], ['Group flash', '15 s', 'Landfall light']],
  },
  see: ['See also', 'Lightship', 'Beacon', 'Buoy', 'Foghorn'],
  footer: 'This page was last edited on 12 March 2026. Text is available under a free licence; additional terms may apply.',
};

// French (a Latin page in another language than English: Latin reader, accents)
const FR = {
  site: "L'encyclopédie libre",
  nav: ['Accueil', 'Modifications récentes', 'Article au hasard', 'Parcourir les thèmes', 'Aide', 'Communauté', 'Faire un don', 'Nous contacter'],
  tools: ['Outils', 'Pages liées', 'Suivi des pages liées', 'Téléverser un fichier', 'Pages spéciales', 'Lien permanent', 'Informations sur la page', 'Citer cette page'],
  title: 'Phare',
  tagline: "De l'encyclopédie libre, le savoir pour tous",
  tabs: ['Article', 'Discussion', 'Lire', 'Modifier', 'Historique'],
  lead: [
    "Un phare est une tour ou une autre construction qui émet une lumière à l'aide d'un système de lampes et de lentilles afin de guider les navires en mer ou sur les eaux intérieures. Les phares signalent les côtes dangereuses, les hauts-fonds, les récifs et les entrées sûres des ports.",
    "Leur nombre a diminué depuis que la navigation par satellite s'est répandue, mais beaucoup fonctionnent encore aujourd'hui, souvent automatisés et surveillés à distance. Certaines vieilles tours sont devenues des musées, des hôtels ou de simples repères sur la côte.",
  ],
  sections: [
    ['Histoire', [
      "Avant la construction des tours, les marins se fiaient à des feux allumés sur les collines près du rivage. Plus tard, des tours de pierre élevèrent la flamme afin qu'elle soit visible de plus loin, et des gardiens vivaient à côté pour entretenir la lumière toute la nuit.",
      "Au dix-neuvième siècle, de nouvelles lentilles faites de nombreux anneaux de verre concentrèrent la lumière en un faisceau puissant, et un mécanisme d'horlogerie faisait tourner la lentille pour que chaque station clignote selon son propre rythme.",
    ]],
    ['Conception', [
      "Une tour typique possède une lanterne au sommet, une galerie autour et un escalier qui monte en spirale dans le fût. Des murs épais protègent la structure du vent et des vagues, et des bandes de peinture la rendent visible le jour.",
    ]],
  ],
  infobox: {
    caption: 'Un phare de pierre sur un cap rocheux',
    rows: [['Type', 'Aide à la navigation'], ['Matériau', 'Pierre, brique ou acier'], ['Hauteur', '10 à 100 mètres'], ['Portée', "Jusqu'à 40 km"], ['Énergie', 'Électrique ou solaire']],
  },
  toc: ['Sommaire', '1 Histoire', '2 Conception', '3 Voir aussi'],
  table: {
    caption: 'Exemples de caractères des feux',
    head: ['Rythme', 'Période', 'Signification'],
    rows: [['Fixe', 'continu', 'Entrée de port'], ['Éclats', '5 s', 'Feu côtier'], ['Occultations', '10 s', 'Marque de chenal'], ['Groupe', '15 s', "Feu d'atterrissage"]],
  },
  see: ['Voir aussi', 'Bateau-phare', 'Balise', 'Bouée', 'Corne de brume'],
  footer: 'Dernière modification de cette page le 12 mars 2026. Le texte est disponible sous licence libre ; des conditions supplémentaires peuvent s’appliquer.',
};

// e: tashkeel, digits (Arabic-Indic and Western), punctuation and brackets
const TASHKEEL = [
  'العِلْمُ نُورٌ يَهْدِي صَاحِبَهُ إِلَى الطَّرِيقِ المُسْتَقِيمِ، وَالجَهْلُ ظُلْمَةٌ تُضِلُّ السَّائِرِينَ.',
  'قَالَ المُعَلِّمُ لِطُلَّابِهِ: اقْرَؤُوا كُلَّ يَوْمٍ صَفْحَةً وَاحِدَةً عَلَى الأَقَلِّ، فَإِنَّ القَلِيلَ الدَّائِمَ خَيْرٌ مِنَ الكَثِيرِ المُنْقَطِعِ.',
  'وَتُزْرَعُ أَشْجَارُ النَّخِيلِ فِي الوَاحَاتِ، وَيُجْنَى ثَمَرُهَا فِي أَوَاخِرِ الصَّيْفِ.',
];
const DIGITS = [
  'بلغ عدد السكان ٤٫٥ مليون نسمة في عام ٢٠٢٤، بزيادة قدرها ٣٪ عن العام السابق.',
  'بلغ عدد الزوار 12,450 زائرًا في شهر 7 من سنة 2025، أي بنسبة 18.5% من الإجمالي.',
  'رقم الهاتف: ٠٥٠١٢٣٤٥٦٧ — الرمز البريدي: 11564 — الصفحات (١٢–٤٨).',
  'انظر الفصل [٣] والملحق (ب)، وقارن ذلك بما ورد في «الكتاب الأول»؛ فهل النتيجة واحدة؟',
  'المعادلة: س = ٢ص + ٧، حيث ص > ٠ و س ≤ ١٠٠.',
];

// g: Arabic lines with English words and numbers inside, English lines with Arabic words
const MIXED_INLINE_AR = [
  'يعمل نظام التشغيل Windows 11 على أجهزة الحاسوب الحديثة، ويدعم تطبيقات Office و Teams.',
  'استخدم الباحثون لغة Python ومكتبة NumPy لتحليل البيانات في عام 2023.',
  'يمكنك زيارة الموقع example.org أو مراسلتنا على البريد info@example.org للمزيد.',
  'سجل الفريق 3 أهداف في الدقيقة 45 من المباراة، وحصل على تقييم 8.7 من 10.',
  'تعتمد تقنية GPS على إشارات الأقمار الصناعية لتحديد الموقع بدقة تصل إلى 5 أمتار.',
  'اضغط على زر Settings ثم اختر Display لتغيير دقة الشاشة إلى 1920x1080.',
];
const MIXED_INLINE_EN = [
  'The word قهوة (qahwa) is the origin of the English word coffee.',
  'In Arabic, the greeting مرحبا means hello, and شكرا means thank you.',
  'The city of دمشق is one of the oldest continuously inhabited cities.',
  'Press the button labelled إرسال to send the form.',
];

// ---- Page builder -----------------------------------------------------------------

function page(t, o) {
  const rtl = o.dir === 'rtl';
  const dark = o.theme === 'dark';
  const c = dark
    ? { bg: '#101418', fg: '#eaecf0', sub: '#a2a9b1', link: '#88a3e8', box: '#1b2027', line: '#3a3f47', head: '#16191d' }
    : { bg: '#ffffff', fg: '#202122', sub: '#54595d', link: '#3366cc', box: '#f8f9fa', line: '#a2a9b1', head: '#ffffff' };
  const body = o.body || 15;
  const small = o.small || Math.max(11, body - 2);
  const font = o.font || "'Segoe UI', Tahoma, Arial, sans-serif";
  const headFont = o.headFont || (o.font ? o.font : "'Times New Roman', serif");
  const a = (x) => `<a href="#">${x}</a>`;
  const nav = (items) => `<ul>${items.map((x) => `<li>${a(x)}</li>`).join('')}</ul>`;
  // a wide screen holds more: the sections again, as a long page shows
  const sections = Array.from({ length: o.repeat || 1 }, () => t.sections).flat()
    .map(([h, ps]) => `<h2>${h}</h2>${ps.map((p) => `<p>${linkify(o.long ? `${p} ${p} ${p}` : p)}</p>`).join('')}`)
    .join('');
  const extra = (o.extra || []).map((x) => `<p>${x}</p>`).join('');
  const blocks = o.blocks || '';
  return `<!doctype html><html lang="${o.lang || (rtl ? 'ar' : 'en')}" dir="${o.dir}"><head><meta charset="utf-8">
<style>
*{box-sizing:border-box} html,body{margin:0;background:${c.bg};color:${c.fg}}
body{font-family:${font};font-size:${body}px;line-height:1.6}
a{color:${c.link};text-decoration:none}
header{display:flex;align-items:center;gap:24px;padding:10px 24px;border-bottom:1px solid ${c.line};background:${c.head}}
.logo{font-family:${headFont};font-size:${body + 7}px;font-weight:bold}
.search{border:1px solid ${c.line};border-radius:2px;padding:4px 10px;width:${o.searchW || 360}px;color:${c.sub};font-size:${body - 1}px}
.hlinks{margin-inline-start:auto;display:flex;gap:16px;font-size:${small}px}
.wrap{display:flex;gap:28px;padding:0 24px;${o.maxw ? `max-width:${o.maxw}px;margin:0 auto;` : ''}}
nav{width:${o.navW || 170}px;flex:none;font-size:${small}px;padding-top:16px}
nav ul{list-style:none;margin:0 0 18px;padding:0} nav li{margin:2px 0}
nav h3{font-size:${small}px;color:${c.sub};margin:0 0 4px;font-weight:normal;border-bottom:1px solid ${c.line}}
main{flex:1;min-width:0}
h1{font-family:${headFont};font-size:${o.h1 || 29}px;font-weight:normal;margin:14px 0 2px;border-bottom:1px solid ${c.line}}
.tabs{display:flex;gap:18px;font-size:${small}px;margin:4px 0}
.tag{font-size:${small}px;color:${c.sub};margin:6px 0 10px}
h2{font-family:${headFont};font-size:${o.h2 || 22}px;font-weight:normal;margin:16px 0 6px;border-bottom:1px solid ${c.line}}
p{margin:6px 0 10px}
.info{float:${rtl ? 'left' : 'right'};width:${o.infoW || 290}px;margin-${rtl ? 'right' : 'left'}:20px;background:${c.box};border:1px solid ${c.line};padding:6px;font-size:${small}px}
.info .img{height:150px;background:linear-gradient(135deg,#6b4f3a,#c49a6c 60%,#e9d8c0);margin-bottom:4px}
.info table{width:100%;border-collapse:collapse} .info th{text-align:start;padding:2px 4px;font-weight:bold} .info td{padding:2px 4px}
.toc{display:inline-block;background:${c.box};border:1px solid ${c.line};padding:6px 14px;font-size:${small}px;margin:6px 0}
.toc div:first-child{font-weight:bold;text-align:center}
table.data{border-collapse:collapse;margin:8px 0;font-size:${small}px}
table.data caption{font-weight:bold;padding:4px}
table.data th,table.data td{border:1px solid ${c.line};padding:4px 12px}
table.data th{background:${c.box}}
.cols{display:flex;gap:40px} .cols>div{flex:1}
footer{clear:both;border-top:1px solid ${c.line};margin-top:16px;padding:8px 0;font-size:${small}px;color:${c.sub}}
</style></head><body>
<header><span class="logo">${t.site}</span><span class="search">${rtl ? 'ابحث في الموسوعة' : 'Search the encyclopedia'}</span>
<span class="hlinks">${a(rtl ? 'إنشاء حساب' : 'Create account')}${a(rtl ? 'دخول' : 'Log in')}</span></header>
<div class="wrap"><nav><h3>${rtl ? 'تصفح' : 'Navigation'}</h3>${nav(t.nav)}<h3>${t.tools[0]}</h3>${nav(t.tools.slice(1))}</nav>
<main><h1>${t.title}</h1><div class="tabs">${t.tabs.map(a).join('')}</div><div class="tag">${t.tagline}</div>
${o.noInfo ? '' : `<div class="info"><div class="img"></div><div>${t.infobox.caption}</div><table>${t.infobox.rows.map(([k, v]) => `<tr><th>${k}</th><td>${v}</td></tr>`).join('')}</table></div>`}
${blocks}
${t.lead.map((p) => `<p>${linkify(p)}</p>`).join('')}${extra}
<div class="toc">${t.toc.map((x) => `<div>${x}</div>`).join('')}</div>
${sections}
<table class="data"><caption>${t.table.caption}</caption><tr>${t.table.head.map((x) => `<th>${x}</th>`).join('')}</tr>${t.table.rows.map((r) => `<tr>${r.map((x) => `<td>${x}</td>`).join('')}</tr>`).join('')}</table>
<h2>${t.see[0]}</h2><ul>${t.see.slice(1).map((x) => `<li>${a(x)}</li>`).join('')}</ul>
<footer>${t.footer}</footer></main></div></body></html>`;
}

// a few words of each paragraph as links (blue), as on an encyclopedia page
function linkify(p) {
  let n = 0;
  return p.replace(/(\S{5,})/g, (w) => (++n % 9 === 4 ? `<a href="#">${w}</a>` : w));
}

const AR_FONTS = {
  'segoe-ui': "'Segoe UI'",
  arial: 'Arial',
  tahoma: 'Tahoma',
  'times-new-roman': "'Times New Roman'",
  'traditional-arabic': "'Traditional Arabic'",
  'arabic-typesetting': "'Arabic Typesetting'",
  'sakkal-majalla': "'Sakkal Majalla'",
  'simplified-arabic': "'Simplified Arabic'",
  aldhabi: 'Aldhabi',
  andalus: 'Andalus',
  'noto-naskh': "'Noto Naskh Arabic'",
  'noto-kufi': "'Noto Kufi Arabic'",
  amiri: 'Amiri',
  cairo: 'Cairo',
};

const FHD = { width: 1920, height: 1080 };
const UW = { width: 3440, height: 1440 };

const mixedBlocks = `<div class="cols"><div dir="ltr" lang="en">${EN.lead.map((p) => `<p>${p}</p>`).join('')}${EN.sections[0][1].map((p) => `<p>${p}</p>`).join('')}</div><div>${AR.lead.map((p) => `<p>${p}</p>`).join('')}${AR.sections[0][1].map((p) => `<p>${p}</p>`).join('')}</div></div>`;
const shortAr = { ...AR, sections: AR.sections.slice(0, 1) };

const PAGES = [
  { name: 'a-ar-light-1080', view: FHD, html: page(AR, { dir: 'rtl', theme: 'light' }) },
  { name: 'a-ar-light-3440', view: UW, html: page(AR, { dir: 'rtl', theme: 'light', searchW: 500, repeat: 3 }) },
  { name: 'a-ar-light-1080-s150', view: FHD, scale: 1.5, html: page(AR, { dir: 'rtl', theme: 'light' }) },
  { name: 'b-ar-dark-1080', view: FHD, html: page(AR, { dir: 'rtl', theme: 'dark' }) },
  { name: 'b-ar-dark-3440', view: UW, html: page(AR, { dir: 'rtl', theme: 'dark', searchW: 500, repeat: 3 }) },
  { name: 'c-ar-small-1080', view: FHD, html: page(AR, { dir: 'rtl', theme: 'light', body: 12, small: 11, h1: 36, h2: 26 }) },
  { name: 'c-ar-small-3440', view: UW, html: page(AR, { dir: 'rtl', theme: 'light', body: 12, small: 11, h1: 36, h2: 26, repeat: 4 }) },
  ...Object.entries(AR_FONTS).map(([k, f]) => ({
    name: `d-font-${k}-1080`,
    view: FHD,
    html: page(shortAr, { dir: 'rtl', theme: 'light', font: f, headFont: f }),
  })),
  // the same fonts at 22 px body: is a failing font too small, or unknown to the reader?
  ...Object.entries(AR_FONTS).map(([k, f]) => ({
    name: `d22-font-${k}-1080`,
    view: FHD,
    html: page(shortAr, { dir: 'rtl', theme: 'light', font: f, headFont: f, body: 22, small: 18 }),
  })),
  { name: 'e-ar-tashkeel-digits-1080', view: FHD, html: page({ ...AR, lead: [], sections: [['التشكيل', TASHKEEL], ['الأرقام وعلامات الترقيم', DIGITS]] }, { dir: 'rtl', theme: 'light', body: 16 }) },
  { name: 'f-mixed-blocks-1080', view: FHD, html: page({ ...AR, lead: [], sections: AR.sections.slice(1, 2) }, { dir: 'rtl', theme: 'light', blocks: mixedBlocks, noInfo: true }) },
  { name: 'g-mixed-inline-1080', view: FHD, html: page({ ...AR, lead: [], sections: [['مصطلحات أجنبية', MIXED_INLINE_AR], ['English with Arabic words', []]] }, { dir: 'rtl', theme: 'light', extra: [], blocks: `<div dir="ltr" lang="en" style="margin-top:12px">${MIXED_INLINE_EN.map((p) => `<p>${p}</p>`).join('')}</div>` }) },
  { name: 'h-en-1080', view: FHD, html: page(EN, { dir: 'ltr', theme: 'light' }) },
  { name: 'h-fr-1080', view: FHD, html: page(FR, { dir: 'ltr', theme: 'light', lang: 'fr' }) },
  { name: 'h-en-3440', view: UW, html: page(EN, { dir: 'ltr', theme: 'light', searchW: 500, repeat: 3 }) },
  // full-width paragraphs (~3100 px lines): wider than the recognizer's input
  { name: 'i-ar-wide-3440', view: UW, html: page(AR, { dir: 'rtl', theme: 'light', searchW: 500, repeat: 2, noInfo: true, long: true }) },
  { name: 'i-en-wide-3440', view: UW, html: page(EN, { dir: 'ltr', theme: 'light', searchW: 500, repeat: 2, noInfo: true, long: true }) },
];

// Every rendered line: the text (DOM = logical order) of the characters that
// sit on one visual line of one block, and the union of their glyph boxes.
function groundTruth() {
  const blockOf = (n) => {
    let e = n.parentElement;
    while (e && getComputedStyle(e).display.startsWith('inline')) e = e.parentElement;
    return e;
  };
  const seg = new Intl.Segmenter(undefined, { granularity: 'grapheme' });
  const blocks = new Map();
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const el = n.parentElement;
    if (!el || el.closest('style')) continue;
    const block = blockOf(n);
    if (!blocks.has(block)) blocks.set(block, []);
    const lines = blocks.get(block);
    const cs = getComputedStyle(el);
    for (const { segment, index } of seg.segment(n.data)) {
      const r = document.createRange();
      r.setStart(n, index);
      r.setEnd(n, index + segment.length);
      const q = [...r.getClientRects()].find((q) => q.width > 0 && q.height > 0);
      if (!q) continue;
      const cy = (q.top + q.bottom) / 2;
      let line = lines.find((l) => cy > l.top && cy < l.bottom);
      if (!line) {
        line = { text: '', left: q.left, top: q.top, right: q.right, bottom: q.bottom, size: parseFloat(cs.fontSize), font: cs.fontFamily, dir: getComputedStyle(block).direction };
        lines.push(line);
      }
      if (/^\s+$/.test(segment)) {
        if (line.text && !line.text.endsWith(' ')) line.text += ' ';
        continue;
      }
      line.text += segment;
      line.left = Math.min(line.left, q.left);
      line.right = Math.max(line.right, q.right);
      line.top = Math.min(line.top, q.top);
      line.bottom = Math.max(line.bottom, q.bottom);
    }
  }
  const out = [];
  for (const lines of blocks.values()) {
    for (const l of lines) {
      const text = l.text.trim();
      if (!text || l.top < 0 || l.left < 0 || l.bottom > innerHeight || l.right > innerWidth) continue;
      out.push({ text, box: [l.left, l.top, l.right - l.left, l.bottom - l.top], size: l.size, font: l.font, dir: l.dir });
    }
  }
  return out;
}

fs.mkdirSync(out, { recursive: true });
process.env.FONTCONFIG_FILE = setupFonts();
const browser = await chromium.launch({ env: { ...process.env } });
const index = [];
for (const p of PAGES) {
  const scale = p.scale || 1;
  const ctx = await browser.newContext({ viewport: p.view, deviceScaleFactor: scale });
  const tab = await ctx.newPage();
  await tab.setContent(p.html, { waitUntil: 'load' });
  await tab.evaluate(() => document.fonts.ready);
  const fonts = await tab.evaluate(() => [...document.fonts].filter((f) => f.status === 'loaded').map((f) => f.family));
  const lines = (await tab.evaluate(groundTruth)).map((l) => ({ ...l, box: l.box.map((v) => Math.round(v * scale * 10) / 10) }));
  await tab.screenshot({ path: path.join(out, `${p.name}.png`) });
  fs.writeFileSync(path.join(out, `${p.name}.html`), p.html);
  fs.writeFileSync(path.join(out, `${p.name}.json`), JSON.stringify({ name: p.name, width: p.view.width * scale, height: p.view.height * scale, scale, lines, fonts }, null, 1));
  index.push(p.name);
  console.log(`${p.name}: ${lines.length} lines`);
  await ctx.close();
}
await browser.close();
fs.writeFileSync(path.join(out, 'index.json'), JSON.stringify(index));
