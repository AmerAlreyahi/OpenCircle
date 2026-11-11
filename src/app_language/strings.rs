//! THE string table: every word Settings and onboarding show, in English
//! (the source) and Arabic. One entry per string:
//!
//!     key: "English"
//!         | "Arabic";
//!
//! To correct a wording, change its text here (nothing else). `{}` is a value
//! filled in at run time (`app_language::fmt`). Keep the keys: code names them.
//! Arabic: Modern Standard Arabic, Western digits, product and technical names
//! in Latin (ARABIC_UI_SPEC.md §6). Seeded from the approved design table
//! (ARABIC_STRINGS.md, 2026-11-11).

/// Builds `S` (one key per entry) and the table (`[English, Arabic]` per
/// key; every Arabic string led by a right-to-left mark).
macro_rules! strings {
    ($($key:ident: $en:literal | $ar:literal;)*) => {
        /// A string of the table (`app_language::tr`).
        #[allow(non_camel_case_types)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u16)]
        pub enum S { $($key),* }

        pub(super) const TABLE: &[[&str; 2]] = &[$([$en, concat!("\u{200F}", $ar)]),*];

        /// Every key's name, in table order (the tests).
        #[cfg(test)]
        pub(super) const KEYS: &[&str] = &[$(stringify!($key)),*];
    };
}

strings! {
    win_title: "OpenCircle Settings"
        | "إعدادات OpenCircle";
    win_pill_one: "1 problem to fix before saving"
        | "مشكلة واحدة يجب إصلاحها قبل الحفظ";
    win_pill_two: "2 problems to fix before saving"
        | "مشكلتان يجب إصلاحهما قبل الحفظ";
    nav_general: "General"
        | "عام";
    nav_appearance: "Appearance"
        | "المظهر";
    nav_look: "Look & behaviour"
        | "الشكل والسلوك";
    nav_ai: "Ask AI"
        | "اسأل الذكاء الاصطناعي";
    nav_text: "Text reading"
        | "قراءة النص";
    nav_translate: "Translate"
        | "الترجمة";
    nav_snip: "Snip"
        | "لقطة الشاشة";
    nav_history: "History"
        | "السجل";
    nav_shortcuts: "Shortcuts"
        | "الاختصارات";
    nav_privacy: "Privacy"
        | "الخصوصية";
    nav_about: "About"
        | "حول";
    nav_labs: "Labs"
        | "ميزات تجريبية";
    nav_exp: "Exp"
        | "تجريبي";
    c_off: "Off"
        | "إيقاف";
    c_on: "On"
        | "تشغيل";
    c_browse: "Browse…"
        | "استعراض…";
    c_reset: "Reset"
        | "إعادة تعيين";
    c_open_history: "Open History"
        | "فتح السجل";
    c_hide_preview: "Hide preview"
        | "إخفاء المعاينة";
    c_show_preview: "Show preview"
        | "إظهار المعاينة";
    c_play_preview: "Play preview"
        | "تشغيل المعاينة";
    c_preview: "Preview"
        | "معاينة";
    c_prev_preview: "Previous preview"
        | "المعاينة السابقة";
    c_next_preview: "Next preview"
        | "المعاينة التالية";
    c_not_set: "Not set"
        | "غير معيّن";
    c_press_keys: "Press keys…"
        | "اضغط المفاتيح…";
    g_lang: "App language"
        | "لغة التطبيق";
    g_lang_desc: "The language of OpenCircle’s windows and menus. Your language is the one you picked in setup"
        | "لغة نوافذ OpenCircle وقوائمه. «لغتك» هي التي اخترتها عند الإعداد";
    // the App language list: "your language" names the language it is now,
    // then each UI language in its own name (`Lang::endonym`: lang_en, lang_ar)
    g_lang_yours: "Your language ({})"
        | "لغتك ({})";
    g_default: "Default action"
        | "الإجراء الافتراضي";
    g_default_desc: "What Enter does after you select something"
        | "ما يفعله Enter بعد أن تحدد شيئًا";
    g_default_search: "Search"
        | "بحث";
    g_default_ai: "Ask AI"
        | "الذكاء الاصطناعي";
    g_default_text: "Text"
        | "نص";
    g_default_copy: "Copy"
        | "نسخ";
    g_start_win: "Start OpenCircle when Windows starts"
        | "تشغيل OpenCircle عند بدء Windows";
    g_start_linux: "Start OpenCircle when you log in"
        | "تشغيل OpenCircle عند تسجيل الدخول";
    g_start_desc: "OpenCircle waits in the tray, with no window"
        | "ينتظر OpenCircle في علبة النظام دون أي نافذة";
    g_folder: "Save folder"
        | "مجلد الحفظ";
    g_folder_desc: "Where Save starts. Files are OpenCircle-date-time.png"
        | "يبدأ الحفظ من هنا. يُسمّى الملف OpenCircle مع التاريخ والوقت";
    g_folder_tip_browse: "Choose the save folder"
        | "اختيار مجلد الحفظ";
    g_update: "Keep OpenCircle up to date"
        | "إبقاء OpenCircle محدّثًا";
    g_update_desc: "Downloads new versions in the background; they install when OpenCircle restarts. Sends only the version request: nothing about you or your screen"
        | "ينزّل الإصدارات الجديدة في الخلفية، وتُثبَّت عند إعادة تشغيل OpenCircle. لا يُرسل سوى طلب الإصدار: لا شيء عنك أو عن شاشتك";
    g_updates_check: "Check now"
        | "التحقق الآن";
    a_reduce: "Reduce animations"
        | "تقليل الحركة";
    a_reduce_off: "Off"
        | "متوقف";
    a_themes: "App and widget themes (12 palettes)"
        | "سمات التطبيق والأدوات (12 لوحة ألوان)";
    a_themes_desc: "The Settings window and every floating widget; a light card turns light mode on"
        | "نافذة الإعدادات وكل أداة عائمة؛ البطاقة الفاتحة تفعّل الوضع الفاتح";
    th_sapphire: "Sapphire"
        | "الياقوت";
    th_sapphire_d: "Deep cobalt blue"
        | "أزرق كوبالتي عميق";
    th_obsidian: "Obsidian"
        | "السَّبَج";
    th_obsidian_d: "Smoked dark charcoal"
        | "فحمي داكن مدخّن";
    th_mica: "Mica Slate"
        | "الأردواز";
    th_mica_d: "Cool slate grey"
        | "رمادي أردوازي بارد";
    th_titanium: "Titanium"
        | "التيتانيوم";
    th_titanium_d: "Warm brushed gunmetal"
        | "معدني مصقول دافئ";
    th_amethyst: "Amethyst"
        | "الجَمَشت";
    th_amethyst_d: "Velvet iris & violet"
        | "بنفسجي مخملي";
    th_emerald: "Emerald"
        | "الزمرّد";
    th_emerald_d: "Deep forest jade"
        | "أخضر يشمي عميق";
    th_rose: "Rose Quartz"
        | "الكوارتز الوردي";
    th_rose_d: "Warm dusk crimson"
        | "قرمزي كالغسق";
    th_graphite: "Graphite"
        | "الجرافيت";
    th_graphite_d: "Calm neutral carbon"
        | "كربوني محايد هادئ";
    th_midnight: "Midnight OLED"
        | "منتصف الليل OLED";
    th_midnight_d: "Pitch-black high contrast"
        | "أسود حالك بتباين عالٍ";
    th_frost: "Arctic Frost"
        | "الصقيع القطبي";
    th_frost_d: "Pale icy blue"
        | "أزرق جليدي فاتح";
    th_daylight: "Daylight"
        | "ضوء النهار";
    th_daylight_d: "Crisp alabaster light"
        | "فاتح مرمري ناصع";
    th_sandstone: "Sandstone"
        | "الحجر الرملي";
    th_sandstone_d: "Warm paper & dune ivory"
        | "عاجي دافئ كالورق";
    a_looks: "Active look screen borders"
        | "حواف الشاشة أثناء العمل";
    a_looks_desc: "The screen edge and the accent while OpenCircle is open"
        | "حافة الشاشة ولون التمييز أثناء فتح OpenCircle";
    lk_glow: "Sapphire Glow"
        | "توهّج الياقوت";
    lk_glow_d: "Sapphire blue to indigo glow round the screen"
        | "توهّج من الأزرق الياقوتي إلى النيلي حول الشاشة";
    lk_glow_b: "Default"
        | "افتراضي";
    lk_spectrum: "Spectrum"
        | "الطيف";
    lk_spectrum_d: "Multi-colour rainbow glow"
        | "توهّج بألوان قوس قزح";
    lk_spectrum_b: "Vivid"
        | "زاهٍ";
    lk_aurora: "Aurora Borealis"
        | "الشفق القطبي";
    lk_aurora_d: "Northern emerald, teal & violet ribbon"
        | "شريط زمردي وفيروزي وبنفسجي";
    lk_aurora_b: "New"
        | "جديد";
    lk_horizon: "Horizon Pulse"
        | "نبض الأفق";
    lk_horizon_d: "Soft light along the top & bottom"
        | "ضوء ناعم على الحافتين العليا والسفلى";
    lk_horizon_b: "Calm"
        | "هادئ";
    lk_sunset: "Sunset Flare"
        | "وهج الغروب";
    lk_sunset_d: "Warm amber, coral & magenta horizon"
        | "أفق كهرماني ومرجاني وأرجواني دافئ";
    lk_sunset_b: "Warm"
        | "دافئ";
    lk_ocean: "Ocean Ripple"
        | "تموّج المحيط";
    lk_ocean_d: "Bioluminescent aqua & deep azure"
        | "أزرق مائي متلألئ ولازوردي عميق";
    lk_ocean_b: "Cool"
        | "بارد";
    lk_neon: "Cyber Neon"
        | "نيون رقمي";
    lk_neon_d: "High-energy electric cyan & hot pink"
        | "سماوي كهربائي ووردي متّقد";
    lk_neon_b: "Vibrant"
        | "نابض";
    lk_solar: "Solar Gold"
        | "الذهب الشمسي";
    lk_solar_d: "Champagne gold & warm white lustre"
        | "ذهبي شمباني بلمعان أبيض دافئ";
    lk_solar_b: "Luxury"
        | "فاخر";
    lk_hud: "Optical HUD"
        | "محدِّد التصويب";
    lk_hud_d: "Precision viewfinder corner brackets"
        | "أقواس زوايا دقيقة كعدسة الكاميرا";
    lk_hud_b: "Minimal"
        | "بسيط";
    lk_custom: "Custom"
        | "مخصّص";
    lk_custom_d: "Your own colours, glow, dim, corners and edges"
        | "ألوانك وتوهّجك وتعتيمك وزواياك وحوافك";
    lk_custom_b: "Yours"
        | "لك";
    cu_colours: "Colours"
        | "الألوان";
    cu_colours_desc: "The screen edge, selection and buttons"
        | "حافة الشاشة والتحديد والأزرار";
    cu_colours_two: "Two colours of my own"
        | "لونان من اختياري";
    cu_two: "Your two colours"
        | "لوناك";
    cu_two_desc: "From the top-left corner round to the opposite one"
        | "من الزاوية العليا اليسرى حتى الزاوية المقابلة";
    cu_glow: "Glow strength"
        | "شدّة التوهّج";
    cu_glow_desc: "How bright the edge glow is"
        | "مدى سطوع توهّج الحافة";
    cu_dim: "Dim"
        | "التعتيم";
    cu_dim_desc: "How much the frozen screen darkens"
        | "مقدار إعتام الشاشة المجمّدة";
    cu_corners: "Selection corners"
        | "زوايا التحديد";
    cu_corners_desc: "The brackets on a snapped box"
        | "الأقواس على المربع الملتقط";
    cu_white: "White"
        | "أبيض";
    cu_accent: "Accent"
        | "لون التمييز";
    cu_style: "Corner style"
        | "شكل الزوايا";
    cu_style_desc: "Four corners, or a frame all round"
        | "أربع زوايا، أو إطار كامل";
    cu_corners_opt: "Corners"
        | "زوايا";
    cu_frame: "Frame"
        | "إطار";
    cu_edge_for: "Screen edge for"
        | "حافة الشاشة في";
    cu_edge_for_desc: "Each mode has its own edge"
        | "لكل وضع حافته";
    cu_circle: "Circle"
        | "الدائرة";
    cu_snip: "Snip"
        | "اللقطة";
    cu_edge: "Edge style"
        | "نمط الحافة";
    cu_edge_desc: "A soft glow along the screen edge, or none"
        | "توهّج ناعم على حافة الشاشة، أو بدونه";
    cu_glow_opt: "Glow"
        | "توهّج";
    cu_none: "None"
        | "بلا";
    l_hero: "Circle to Search"
        | "البحث بالدائرة";
    l_hero_desc: "Circle anything and search in your browser."
        | "أحِط أي شيء بدائرة وابحث عنه في متصفحك.";
    l_sel: "Selection"
        | "التحديد";
    l_sel_desc: "Snap: a rounded box with corners to drag"
        | "الالتقاط: مربع مستدير الزوايا تسحب أركانه";
    l_sel_snap: "Snap to box"
        | "التقاط إلى مربع";
    l_sel_drawn: "As drawn"
        | "كما رُسم";
    l_themes: "Themes and screen borders"
        | "السمات وحواف الشاشة";
    l_themes_desc: "All 12 themes and the screen border looks live in Appearance"
        | "السمات الاثنتا عشرة وأشكال حواف الشاشة كلها في «المظهر»";
    ai_hero_desc: "Ask a question about what is on your screen."
        | "اطرح سؤالًا عمّا تعرضه شاشتك.";
    ai_from: "Answers from"
        | "مصدر الإجابات";
    ai_from_browser: "Google AI Mode in your browser. No key"
        | "Google AI Mode في متصفحك، دون مفتاح";
    ai_from_desc: "Who answers your questions"
        | "من يجيب عن أسئلتك";
    ai_browser: "Browser"
        | "المتصفح";
    ai_provider: "AI provider"
        | "مزوّد ذكاء اصطناعي";
    ai_prov: "Provider"
        | "المزوّد";
    ai_prov_desc: "The service that answers, with your key"
        | "الخدمة التي تجيب، بمفتاحك";
    ai_prov_choose: "Choose a provider"
        | "اختر مزوّدًا";
    ai_url: "Server URL"
        | "عنوان الخادم";
    ai_url_desc: "Your own server or any compatible API"
        | "خادمك الخاص أو أي واجهة API متوافقة";
    ai_url_ph: "https://my-server.example/v1"
        | "https://my-server.example/v1";
    ai_model: "Model"
        | "النموذج";
    ai_model_desc: "The model id sent to the provider"
        | "معرّف النموذج الذي يُرسَل إلى المزوّد";
    ai_model_eg: "e.g. {}"
        | "مثل {}";
    ai_key: "API key"
        | "مفتاح API";
    ai_key_desc: "Kept in your system keyring, never in a file"
        | "يُحفظ في مخزن مفاتيح النظام، ولا يُكتب في ملف أبدًا";
    ai_key_local: "Not needed for a server on this PC"
        | "غير مطلوب لخادم على هذا الجهاز";
    ai_key_ph: "Paste your API key"
        | "الصق مفتاح API";
    ai_key_save: "Save key"
        | "حفظ المفتاح";
    ai_key_replace: "Replace"
        | "استبدال";
    ai_key_remove: "Remove"
        | "إزالة";
    ai_stays: "AI chat stays open"
        | "إبقاء محادثة الذكاء الاصطناعي مفتوحة";
    ai_stays_desc: "Keep the chat above other apps. Its pin unpins and closes it"
        | "تبقى المحادثة فوق التطبيقات الأخرى، ودبّوسها يلغي التثبيت ويغلقها";
    ai_stays_follow: "After a follow-up"
        | "بعد سؤال متابعة";
    ai_stays_always: "Always"
        | "دائمًا";
    ai_hist: "Chat history"
        | "سجل المحادثات";
    ai_hist_on: "On: chats, text only, kept on this PC. Kept and cleared in History"
        | "مفعّل: المحادثات نصًّا فقط، محفوظة على هذا الجهاز. تُدار وتُمسح من «السجل»";
    t_hero_desc: "Select screen text and copy it into any app."
        | "حدّد نص الشاشة وانسخه إلى أي تطبيق.";
    t_engine: "Engine"
        | "المحرّك";
    t_auto: "Auto"
        | "تلقائي";
    t_fast: "Fast"
        | "سريع";
    t_accurate: "Accurate"
        | "دقيق";
    t_add: "Add in Windows"
        | "إضافة في Windows";
    tr_hero_desc: "Read your screen in your language, in place."
        | "اقرأ شاشتك بلغتك، في مكانها.";
    tr_engine: "Engine"
        | "المحرّك";
    tr_engine_local: "No key, nothing leaves your PC; more literal than AI"
        | "بلا مفتاح، ولا يغادر جهازك شيء؛ أكثر حرفية من الذكاء الاصطناعي";
    tr_ai: "AI"
        | "الذكاء الاصطناعي";
    tr_local: "Local"
        | "محلي";
    tr_source: "Source language"
        | "لغة المصدر";
    tr_source_desc: "What the text on screen is in; picks the text recognizer"
        | "لغة النص على الشاشة؛ وبها يُختار قارئ النص";
    tr_detect: "Detect automatically"
        | "اكتشاف تلقائي";
    tr_target: "Target language"
        | "اللغة الهدف";
    tr_target_desc: "What the text is translated into"
        | "اللغة التي يُترجم إليها النص";
    lang_ar: "Arabic"
        | "العربية";
    lang_en: "English"
        | "الإنجليزية";
    lang_fr: "French"
        | "الفرنسية";
    lang_de: "German"
        | "الألمانية";
    lang_es: "Spanish"
        | "الإسبانية";
    lang_pt: "Portuguese"
        | "البرتغالية";
    lang_it: "Italian"
        | "الإيطالية";
    lang_nl: "Dutch"
        | "الهولندية";
    lang_ru: "Russian"
        | "الروسية";
    lang_uk: "Ukrainian"
        | "الأوكرانية";
    lang_pl: "Polish"
        | "البولندية";
    lang_tr: "Turkish"
        | "التركية";
    lang_fa: "Persian"
        | "الفارسية";
    lang_ur: "Urdu"
        | "الأردية";
    lang_he: "Hebrew"
        | "العبرية";
    lang_hi: "Hindi"
        | "الهندية";
    lang_id: "Indonesian"
        | "الإندونيسية";
    lang_vi: "Vietnamese"
        | "الفيتنامية";
    lang_th: "Thai"
        | "التايلاندية";
    lang_ja: "Japanese"
        | "اليابانية";
    lang_ko: "Korean"
        | "الكورية";
    lang_pt_br: "Portuguese (Brazil)"
        | "البرتغالية (البرازيل)";
    lang_zh_hant: "Chinese (Traditional)"
        | "الصينية (التقليدية)";
    lang_zh: "Chinese (Simplified)"
        | "الصينية (المبسّطة)";
    s_hero_desc: "Capture, mark up and copy a picture."
        | "التقط صورة، ودوّن عليها، وانسخها.";
    s_row_desc: "Click a window or drag an area"
        | "انقر نافذة أو اسحب لتحديد منطقة";
    s_copy: "Copy when picked"
        | "النسخ عند التحديد";
    s_copy_desc: "The picked area is also copied, ready to paste into an AI chat"
        | "تُنسخ المنطقة المحددة أيضًا، جاهزة للصق في محادثة ذكاء اصطناعي";
    s_size: "Image size"
        | "حجم الصورة";
    s_size_desc: "Long edge for Copy and Save; AI chats shrink bigger ones anyway"
        | "الضلع الأطول عند النسخ والحفظ؛ محادثات الذكاء الاصطناعي تصغّر الأكبر منه على أي حال";
    s_original: "Original"
        | "الأصلي";
    s_as: "Save as"
        | "الحفظ بصيغة";
    s_as_desc: "The file Save writes"
        | "نوع الملف الذي يكتبه الحفظ";
    s_png: "PNG"
        | "PNG";
    s_jpeg: "JPEG"
        | "JPEG";
    s_save_size: "Save size"
        | "حجم الحفظ";
    s_save_size_desc: "Copy always uses the image size"
        | "يستخدم النسخ دائمًا «حجم الصورة»";
    s_pointer: "Mouse pointer"
        | "مؤشر الماوس";
    s_pointer_desc: "Draw the pointer into the capture"
        | "رسم المؤشر في اللقطة";
    s_timer: "Timer"
        | "المؤقّت";
    s_timer_desc: "Wait before a snip captures, so you can open a menu first"
        | "الانتظار قبل الالتقاط، لتتمكن من فتح قائمة أولًا";
    s_t3: "3 s"
        | "3 ث";
    s_t5: "5 s"
        | "5 ث";
    s_t10: "10 s"
        | "10 ث";
    s_long: "Long capture"
        | "اللقطة الطويلة";
    s_long_desc: "How tall a long capture may grow; taller takes more memory"
        | "أقصى طول للقطة الطويلة بالبكسل؛ الأطول يستهلك ذاكرة أكثر";
    s_px5: "5,000 px"
        | "5,000";
    s_px10: "10,000 px"
        | "10,000";
    s_px20: "20,000 px"
        | "20,000";
    s_recent: "Recent captures"
        | "اللقطات الأخيرة";
    s_recent_on: "On: the last 20, on this PC only. Kept and cleared in History"
        | "مفعّل: آخر 20، على هذا الجهاز فقط. تُدار وتُمسح من «السجل»";
    h_captures_desc: "The last 20 snips, on this PC only. Off deletes them"
        | "آخر 20 لقطة، على هذا الجهاز فقط. الإيقاف يحذفها";
    h_chats_desc: "AI chats, text only, on this PC. Off deletes them"
        | "محادثات الذكاء الاصطناعي، نصًّا فقط، على هذا الجهاز. الإيقاف يحذفها";
    h_kept: "{} kept"
        | "المحفوظ: {}";
    h_none: "Nothing kept"
        | "لا شيء محفوظ";
    h_clear: "Clear"
        | "مسح";
    h_all: "Clear all"
        | "مسح الكل";
    h_all_desc: "Every kept capture and chat on this PC"
        | "كل اللقطات والمحادثات المحفوظة على هذا الجهاز";
    h_all_btn: "Clear all…"
        | "مسح الكل…";
    k_note: "Click any box to record keys"
        | "انقر أي مربع لتسجيل المفاتيح";
    k_open: "Open OpenCircle"
        | "فتح OpenCircle";
    k_open_desc: "Starts a capture"
        | "يبدأ الالتقاط";
    k_whole: "Whole screen"
        | "الشاشة كاملة";
    k_explain: "Explain with AI"
        | "الشرح بالذكاء الاصطناعي";
    k_circle: "The area you circle"
        | "المنطقة التي تحيطها بدائرة";
    k_copytext: "Copy text"
        | "نسخ النص";
    k_lens: "Search with Lens"
        | "البحث باستخدام Lens";
    k_colour: "Pick a colour"
        | "التقاط لون";
    k_colour_desc: "Click copies HEX, Shift+click RGB"
        | "النقر ينسخ HEX، وShift+نقر ينسخ RGB";
    k_ruler: "Measure the screen"
        | "قياس الشاشة";
    k_ruler_desc: "Drag to measure in pixels"
        | "اسحب للقياس بالبكسل";
    k_cancel: "Esc cancels · Delete clears"
        | "Esc للإلغاء · Delete للمسح";
    k_tip: "{} shortcut"
        | "اختصار {}";
    p_note: "Nothing leaves your PC until you choose an action. No telemetry or analytics."
        | "لا يغادر جهازك شيء حتى تختار إجراءً. لا قياس عن بُعد ولا تحليلات.";
    p_h1: "Feature"
        | "الميزة";
    p_h2: "What leaves your PC"
        | "ما يغادر جهازك";
    p_h3: "Where it goes"
        | "وجهته";
    p_r1a: "Search in browser"
        | "البحث في المتصفح";
    p_r1b: "The selected area, or only the text you selected"
        | "المنطقة المحددة، أو النص الذي حددته فقط";
    p_r1c: "Google Lens or Search, in your default browser"
        | "Google Lens أو بحث Google، في متصفحك الافتراضي";
    p_r2a: "Ask AI, no key"
        | "اسأل الذكاء الاصطناعي، بلا مفتاح";
    p_r2c: "Google AI Mode, in your default browser"
        | "Google AI Mode، في متصفحك الافتراضي";
    p_r3a: "Ask AI, your key"
        | "اسأل الذكاء الاصطناعي، بمفتاحك";
    p_r3b: "Your question and the selected area"
        | "سؤالك والمنطقة المحددة";
    p_r3c: "Your AI provider, or your own server"
        | "مزوّد الذكاء الاصطناعي لديك، أو خادمك الخاص";
    p_r4a: "Translate with AI"
        | "الترجمة بالذكاء الاصطناعي";
    p_r4b: "The text read from the screen"
        | "النص المقروء من الشاشة";
    p_r4c: "Your AI provider"
        | "مزوّد الذكاء الاصطناعي لديك";
    p_r5a: "Text reading (OCR)"
        | "قراءة النص (OCR)";
    p_nothing: "Nothing"
        | "لا شيء";
    p_here: "Runs on this PC"
        | "يعمل على هذا الجهاز";
    p_r6a: "Local translate"
        | "الترجمة المحلية";
    p_r7a: "Updates"
        | "التحديثات";
    p_r7b: "Only the request for the latest version (when on)"
        | "طلب أحدث إصدار فقط (عند التفعيل)";
    p_r7c: "GitHub Releases"
        | "GitHub Releases";
    ab_stack: "Pure Rust · iced"
        | "Pure Rust · iced";
    ab_mit: "MIT"
        | "MIT";
    ab_what: "Open-source desktop Circle to Search for Windows & Linux: circle anything on screen to search in your browser, read text (OCR), translate in place, or ask AI."
        | "«البحث بالدائرة» لسطح المكتب، مفتوح المصدر، على Windows وLinux: أحِط أي شيء على الشاشة بدائرة لتبحث عنه في متصفحك، أو تقرأ نصه (OCR)، أو تترجمه في مكانه، أو تسأل عنه الذكاء الاصطناعي.";
    ab_github: "GitHub"
        | "GitHub";
    ab_website: "Website"
        | "الموقع";
    ab_licence: "MIT licence"
        | "رخصة MIT";
    // English: the bio of assets/about/profile.toml ({}); Arabic: the draft
    // (the owner corrects it). The name stays as the profile writes it.
    ab_bio: "{}"
        | "مهندس حلول في الذكاء الاصطناعي وإدارة المحافظ والمشاريع، يبني أدوات سطح مكتب سريعة تحترم الخصوصية.";
    ab_profile: "GitHub profile"
        | "الملف على GitHub";
    ab_star: "Star OpenCircle"
        | "ضع نجمة لـ OpenCircle";
    ab_others: "Made by others"
        | "من صنع الآخرين";
    ab_others_desc: "Downloaded on first use, under their own licences"
        | "تُنزَّل عند أول استخدام، بتراخيصها الخاصة";
    ab_all: "All licences"
        | "كل التراخيص";
    ab_n1: "Text recognition"
        | "التعرّف على النص";
    ab_n2: "OCR ONNX export"
        | "تصدير OCR بصيغة ONNX";
    ab_n3: "ONNX Runtime"
        | "ONNX Runtime";
    ab_n4: "Local translation"
        | "الترجمة المحلية";
    ab_n4b: "OPUS-MT, Helsinki-NLP; ONNX by Xenova"
        | "OPUS-MT من Helsinki-NLP؛ وONNX من Xenova";
    ab_n5b: "The iced contributors"
        | "المساهمون في iced";
    ab_n8: "Lucide icons"
        | "أيقونات Lucide";
    ab_n8b: "Lucide contributors"
        | "المساهمون في Lucide";
    ab_n9: "Inter font"
        | "خط Inter";
    ab_clips: "Preview clips"
        | "مقاطع المعاينة";
    ab_problems: "Problems"
        | "المشكلات";
    ab_problems_desc: "Copy a report to paste into an issue. Nothing is sent"
        | "انسخ تقريرًا لتلصقه في بلاغ. لا يُرسَل شيء";
    ab_diag: "Copy diagnostics"
        | "نسخ التشخيص";
    ab_report: "Report a problem"
        | "الإبلاغ عن مشكلة";
    ab_telemetry: "Telemetry"
        | "القياس عن بُعد";
    ab_telemetry_desc: "A capture leaves this PC only when you choose an action"
        | "لا تغادر اللقطة هذا الجهاز إلا حين تختار إجراءً";
    ab_none: "None"
        | "لا يوجد";
    lb_note: "All off by default · Hidden when off"
        | "كلها متوقفة افتراضيًا · تختفي عند إيقافها";
    lb_tag: "Experimental"
        | "تجريبية";
    lb_song: "Song search"
        | "البحث عن أغنية";
    lb_song_d: "Find the song playing on your PC. Unfinished; may change"
        | "تعرّف على الأغنية التي تُشغَّل على جهازك. غير مكتملة؛ قد تتغير";
    lb_pins: "Pins"
        | "التثبيت الحيّ";
    lb_pins_d: "Keep a part of a window live above your windows. Unfinished; may change"
        | "أبقِ جزءًا من نافذة حيًّا فوق نوافذك. غير مكتملة؛ قد تتغير";
    lb_live: "Live translate"
        | "الترجمة الحيّة";
    lb_live_d: "Keep the screen translated as it changes. Unfinished; may change"
        | "أبقِ الشاشة مترجمة أثناء تغيّرها. غير مكتملة؛ قد تتغير";
    o_title: "Welcome to OpenCircle"
        | "مرحبًا بك في OpenCircle";
    // a step's number and name: the number isolated, so it reads first
    // (right) in Arabic, its dot after it (the design render showed ".5")
    o_step: "{}. {}"
        | "{}. {}";
    o_s_language: "Language"
        | "اللغة";
    o_s_welcome: "Welcome"
        | "الترحيب";
    o_s_ai: "Connect AI"
        | "ربط الذكاء الاصطناعي";
    o_s_look: "Look"
        | "المظهر";
    o_s_done: "Done"
        | "الانتهاء";
    o_s_shots: "Screenshots"
        | "لقطات الشاشة";

    o_skip: "Skip"
        | "تخطٍّ";
    o_finish: "Finish"
        | "إنهاء";
    o_lang: "Your language"
        | "لغتك";
    // (the app speaks only English and Arabic: "and uses it for the app" is
    // said only where it is true, so the English line stays as it was)
    o_lang_desc: "OpenCircle translates text on your screen into it. You can change it any time in Settings > Translate."
        | "يترجم OpenCircle النص على شاشتك إليها. يمكنك تغييرها متى شئت في الإعدادات، صفحة «الترجمة».";
    o_model_en: "Text in another language is translated into English with that language's local model, downloaded the first time you translate it. Nothing is downloaded or sent before that."
        | "يُترجَم النص المكتوب بلغة أخرى إلى الإنجليزية بنموذج تلك اللغة المحلي، ويُنزَّل أول مرة تترجمها. لا يُنزَّل أو يُرسَل شيء قبل ذلك.";
    o_model_pair: "{} → {} local translation"
        | "نموذج الترجمة المحلية من {} إلى {}";
    o_model_dl: "Your first translation downloads {} (about {} MB). Nothing is downloaded or sent before that."
        | "أول ترجمة تنزّل {} (نحو {} ميغابايت). لا يُنزَّل أو يُرسَل شيء قبل ذلك.";
    o_model_here: "{} is already on this PC."
        | "{} موجود على هذا الجهاز.";
    o_model_into: "Your first translation into {} downloads its local model. Nothing is downloaded or sent before that."
        | "أول ترجمة إلى {} تنزّل نموذجها المحلي. لا يُنزَّل أو يُرسَل شيء قبل ذلك.";
    o_model_none: "No local model translates into {} yet: choose AI in Settings > Translate to translate into it."
        | "لا يوجد بعد نموذج محلي يترجم إلى {}: اختر الذكاء الاصطناعي في الإعدادات، صفحة «الترجمة»، للترجمة إليها.";

    o_welcome_desc: "Press a shortcut, circle anything on your screen, then choose whether to search in your browser, ask AI, copy text or translate."
        | "اضغط اختصارًا، وأحِط أي شيء على شاشتك بدائرة، ثم اختر: البحث في متصفحك، أو سؤال الذكاء الاصطناعي، أو نسخ النص، أو الترجمة.";
    o_shots: "Allow screenshots"
        | "السماح بلقطات الشاشة";
    o_shots_desc: "Your desktop asks once whether OpenCircle may capture the screen, and only while an OpenCircle window is in front. Click Allow, then Share (or Allow) in its dialog."
        | "يسألك سطح المكتب مرة واحدة إن كان OpenCircle يستطيع التقاط الشاشة، وذلك فقط حين تكون إحدى نوافذه في المقدمة. انقر «سماح»، ثم «مشاركة» (أو «سماح») في مربع الحوار الذي يظهر.";
    o_allow: "Allow"
        | "سماح";
    o_allow_tip: "Ask the desktop for screenshot permission"
        | "طلب إذن لقطات الشاشة من سطح المكتب";
    o_once: "You are asked once; nothing is saved."
        | "يُطلب منك ذلك مرة واحدة؛ ولا يُحفظ شيء.";
    o_done_desc: "Ready whenever you need to search, read, translate or ask about your screen"
        | "جاهز متى احتجت إلى البحث أو القراءة أو الترجمة أو السؤال عن شاشتك";
    // ---------- added with the build (not in the design table: owner's check) ----------
    win_pill_few: "{} problems to fix before saving"
        | "{} مشكلات يجب إصلاحها قبل الحفظ";
    win_pill_many: "{} problems to fix before saving"
        | "{} مشكلة يجب إصلاحها قبل الحفظ";
    win_ask_close: "Save your changes before closing?"
        | "هل تحفظ تغييراتك قبل الإغلاق؟";
    win_discard: "Discard"
        | "تجاهل";
    win_keep_editing: "Keep editing"
        | "متابعة التعديل";
    win_save: "Save"
        | "حفظ";
    p_song_a: "Song search (Labs)"
        | "البحث عن الأغاني (ميزة تجريبية)";
    p_song_b: "A ~10-second clip, only when you press the music button"
        | "مقطع مدته 10 ثوانٍ تقريبًا، فقط عندما تضغط زر الموسيقى";
    p_song_c: "AudD (with your token)"
        | "AudD (برمزك الخاص)";
    o_back_arrow: "← Back"
        | "→ رجوع";
    o_next_arrow: "Next →"
        | "التالي ←";
    o_fix_first: "Fix this first: {}"
        | "أصلح هذا أولًا: {}";
    o_save_failed: "Could not save your settings: {}"
        | "تعذّر حفظ إعداداتك: {}";
    lb_live_hero: "Follow game and video dialogue in your language."
        | "تابع حوارات الألعاب والفيديو بلغتك.";
    lb_song_hero: "Name a song playing on your PC or microphone."
        | "تعرّف على أغنية تُشغَّل على جهازك أو يلتقطها الميكروفون.";
    c_preview_failed: "Couldn't download the preview. Check your connection and try again."
        | "تعذّر تنزيل المعاينة. تحقق من اتصالك ثم حاول مرة أخرى.";
    c_downloading: "Downloading…"
        | "جارٍ التنزيل…";
    c_no_shortcut: "Shortcut not set"
        | "لم يُعيَّن اختصار";
    ai_key_save_tip: "Save the API key"
        | "حفظ مفتاح API";
    ai_key_replace_tip: "Replace the API key"
        | "استبدال مفتاح API";
    ai_key_remove_tip: "Remove the API key"
        | "إزالة مفتاح API";
    ai_key_keep_tip: "Keep the saved API key"
        | "الإبقاء على مفتاح API المحفوظ";
    song_key_ph: "Paste your AudD API token"
        | "الصق رمز AudD API الخاص بك";
    song_key: "AudD API token"
        | "رمز AudD API";
    song_key_save_tip: "Save the AudD token"
        | "حفظ رمز AudD";
    song_key_replace_tip: "Replace the AudD token"
        | "استبدال رمز AudD";
    song_key_remove_tip: "Remove the AudD token"
        | "إزالة رمز AudD";
    song_key_keep_tip: "Keep the saved AudD token"
        | "الإبقاء على رمز AudD المحفوظ";
    key_checking: "Checking…"
        | "جارٍ التحقق…";
    key_saving: "Saving…"
        | "جارٍ الحفظ…";
    key_removing: "Removing…"
        | "جارٍ الإزالة…";
    key_paste_first: "Paste your key first"
        | "الصق مفتاحك أولًا";
    g_restart_now: "Restart now"
        | "إعادة التشغيل الآن";
    g_get_it: "Get it"
        | "احصل عليه";
    g_whats_new: "What's new"
        | "ما الجديد";
    g_folder_system: "Chosen by your system"
        | "يختاره نظامك";
    g_folder_tip_reset: "Use the system's folder"
        | "استخدام مجلد النظام";
    a_reduce_win: "Follows Windows' Show animations setting: off there, pages and widgets move without transitions"
        | "يتبع إعداد «إظهار الحركات» في Windows: عند إيقافه تتبدّل الصفحات والأدوات دون انتقالات";
    a_reduce_linux: "Follows your desktop's animation setting: off there, pages and widgets move without transitions"
        | "يتبع إعداد الحركات في سطح المكتب: عند إيقافه تتبدّل الصفحات والأدوات دون انتقالات";
    l_themes_open_arrow: "Open Appearance →"
        | "فتح «المظهر» ←";
    ai_hist_off: "Off: no chat is kept. Turn it on in History"
        | "متوقف: لا تُحفظ أي محادثة. فعّله من «السجل»";
    ai_model_type: "Type or load a model id"
        | "اكتب مُعرّف نموذج أو حمّل القائمة";
    c_cancel: "Cancel"
        | "إلغاء";
    t_engine_desc_src: "Auto picks the best for the language on screen. Fast: Windows' own; Accurate: PaddleOCR"
        | "«تلقائي» يختار الأنسب للغة الظاهرة على الشاشة. «سريع»: قارئ Windows نفسه؛ «دقيق»: PaddleOCR";
    t_note_lang: "Windows has no text recognition for {}. Using the downloaded reader."
        | "لا يتوفر في Windows تعرّف على نصوص {}. يُستخدم القارئ الذي نُزّل.";
    tr_engine_ai: "Your AI provider; local when it isn't set up"
        | "مزوّد الذكاء الاصطناعي لديك؛ وتُستخدم الترجمة المحلية إن لم يكن معدًّا";
    tr_no_model: "No local model for this pair yet (coming in a future update): choose AI to translate it"
        | "لا يوجد نموذج محلي لهذا الزوج بعد (يأتي في تحديث قادم): اختر الذكاء الاصطناعي لترجمته";
    tr_live_tip: "Live translate refresh interval"
        | "فاصل تحديث الترجمة الحية";
    tr_live: "Live refresh"
        | "التحديث الحي";
    tr_live_desc: "Milliseconds between updates in live translate"
        | "المدة بين تحديثات الترجمة الحية بالمللي ثانية";
    s_prtsc: "Print Screen key"
        | "مفتاح Print Screen";
    s_prtsc_settings: "Open Windows settings"
        | "فتح إعدادات Windows";
    s_recent_off: "Off: nothing is kept. Turn it on in History"
        | "متوقف: لا يُحفظ شيء. فعّله من «السجل»";
    h_counting: "Counting…"
        | "جارٍ العدّ…";
    h_delete_all: "Delete all"
        | "حذف الكل";
    h_delete_ask: "Delete every kept capture and chat? This can't be undone"
        | "هل تحذف كل اللقطات والمحادثات المحفوظة؟ لا يمكن التراجع عن ذلك";
    k_open_tip: "Open OpenCircle shortcut"
        | "اختصار فتح OpenCircle";
    k_esc: "Esc cancels"
        | "Esc يلغي";
    cu_choose: "Choose colours"
        | "اختر الألوان";
    cu_look_colours: "Look colours"
        | "ألوان المظهر";
    cu_first: "First colour"
        | "اللون الأول";
    cu_second: "Second colour"
        | "اللون الثاني";
    cu_dim_tip: "Dim strength"
        | "قوة التعتيم";
    cu_bars: "Bars and pills"
        | "الأشرطة والأزرار";
    cu_bars_desc: "The action bar, chips and panels"
        | "شريط الإجراءات والشارات واللوحات";
    cu_style_choose: "Choose a style"
        | "اختر نمطًا";
    cu_bar_style: "Bar style"
        | "نمط الشريط";
    cu_live: "Live"
        | "الحي";
    cu_aurora: "Aurora"
        | "الشفق";
    cu_sunset: "Sunset"
        | "الغروب";
    cu_ocean: "Ocean"
        | "المحيط";
    cu_neon: "Neon"
        | "نيون";
    cu_mono: "Mono"
        | "أحادي";
    cu_glass: "Glass (app theme)"
        | "زجاج (سمة التطبيق)";
    cu_glass_dark: "Dark glass"
        | "زجاج داكن";
    cu_glass_light: "Light glass"
        | "زجاج فاتح";
    cu_solid: "Solid"
        | "مُصمت";
    cu_outline: "Outline"
        | "حدّ خارجي";
    song_privacy: "When you press the music button, a ~10-second clip of the audio you choose is sent to AudD. Nothing is recorded at any other time."
        | "عندما تضغط زر الموسيقى، يُرسَل مقطع مدته 10 ثوانٍ تقريبًا من الصوت الذي تختاره إلى AudD. لا يُسجَّل شيء في أي وقت آخر.";
    song_service: "Service"
        | "الخدمة";
    song_need_key: "Free fingerprint lookups can't name a short clip, so this needs your own key"
        | "البحث المجاني بالبصمة لا يتعرّف على مقطع قصير، لذا تحتاج هذه الميزة إلى مفتاحك الخاص";
    song_sign_up: "Sign up at dashboard.audd.io and paste your token. Kept in your system keyring"
        | "سجّل في dashboard.audd.io والصق رمزك. يُحفظ في مخزن مفاتيح النظام";
    song_listen: "Listen to"
        | "الاستماع إلى";
    song_pc_desc: "What this PC plays: a video, a game, a stream"
        | "ما يشغّله هذا الجهاز: فيديو أو لعبة أو بث";
    song_mic_desc: "Your microphone: music playing near you"
        | "الميكروفون: الموسيقى التي تُعزف بالقرب منك";
    song_pc: "This PC"
        | "هذا الجهاز";
    song_mic: "Microphone"
        | "الميكروفون";
    song_recent: "Recent songs, kept only on this PC. Off deletes them"
        | "الأغاني الأخيرة، محفوظة على هذا الجهاز فقط. الإيقاف يحذفها";
    k_translate_live: "Translate (live, Beta)"
        | "ترجمة (حية، تجريبية)";
    k_last_region: "The last region"
        | "آخر منطقة";
    k_window: "The window you click"
        | "النافذة التي تنقر عليها";
    k_song_desc: "Listens and names the song (needs a key)"
        | "يستمع ويتعرّف على الأغنية (يحتاج إلى مفتاح)";
    k_pin_desc: "Pick a part of a window: it floats on top, kept up to date"
        | "اختر جزءًا من نافذة: يطفو فوق النوافذ ويبقى محدَّثًا";
    k_prtsc_windows: "Windows' screen capture uses this key"
        | "أداة التقاط الشاشة في Windows تستخدم هذا المفتاح";
    k_in_use: "In use: {}. {}"
        | "مستخدم: {}. {}";
    k_used_by: "Used by {}"
        | "يستخدمه {}";
    k_already: "Already used by {}"
        | "مستخدم بالفعل في {}";
    k_taken: "Taken by another app: pick another key"
        | "يستخدمه تطبيق آخر: اختر مفتاحًا آخر";
    k_no_service: "No shortcut service here: bind \"{}\" in your desktop's keyboard settings"
        | "لا توجد هنا خدمة اختصارات: اربط «{}» في إعدادات لوحة المفاتيح في سطح المكتب";
    win_invalid: "Some saved settings were invalid ({})"
        | "بعض الإعدادات المحفوظة كانت غير صالحة ({})";
    win_fix_first: "Fix the problems above first"
        | "أصلح المشكلات أعلاه أولًا";
    win_nothing: "Nothing to save"
        | "لا شيء للحفظ";
    win_save_failed: "Could not save: {}"
        | "تعذّر الحفظ: {}";
    o_waiting: "Waiting…"
        | "بانتظار الرد…";
    o_allowed: "Allowed"
        | "مسموح";
    o_answer: "Answer the dialog your desktop shows."
        | "أجب عن مربع الحوار الذي يعرضه سطح المكتب.";
    o_can_capture: "OpenCircle can capture your screen now."
        | "يستطيع OpenCircle الآن التقاط شاشتك.";
    o_said_no: "Your desktop said no. Try again, or allow it later."
        | "رفض سطح المكتب الطلب. حاول مرة أخرى، أو اسمح به لاحقًا.";
    o_not_allowed: "Not allowed ({}). Try again, or allow it later."
        | "غير مسموح ({}). حاول مرة أخرى، أو اسمح به لاحقًا.";
    o_needs_shots: "OpenCircle needs screenshot permission"
        | "يحتاج OpenCircle إلى إذن التقاط الشاشة";
    o_done_press: "OpenCircle runs in the tray. Press {}."
        | "يعمل OpenCircle في علبة النظام. اضغط {}.";
    o_shots_again: "Your desktop asks only while an OpenCircle window is in front. Click Allow, then Share (or Allow) in its dialog, and press your shortcut again."
        | "يسأل سطح المكتب فقط حين تكون إحدى نوافذ OpenCircle في المقدمة. انقر «سماح»، ثم «مشاركة» (أو «سماح») في مربع الحوار، ثم اضغط اختصارك مرة أخرى.";
    s_prtsc_desc: "Windows opens its own screen capture with it. To snip with Print Screen, turn off \"Use the Print screen key to open screen capture\" in Windows"
        | "يفتح Windows أداة التقاط الشاشة الخاصة به بهذا المفتاح. لالتقاط الشاشة بمفتاح Print Screen، أوقف في Windows الخيار «Use the Print screen key to open screen capture»";
    song_token: "API token"
        | "رمز API";
    k_live_pin: "Live pin"
        | "التثبيت الحي";

    // ---- added in part 2a (tray, update widgets, pills, model downloads,
    // translate pill and picker, backend states): for the owner's native review ----
    tray_circle: "Circle to Search"
        | "البحث بالدائرة";
    tray_translate: "Translate screen"
        | "ترجمة الشاشة";
    tray_click_pins: "Make pins clickable"
        | "جعل المثبّتات قابلة للنقر";
    tray_settings: "Settings…"
        | "الإعدادات…";
    tray_quit: "Quit"
        | "خروج";
    tray_update: "Restart to update"
        | "أعد التشغيل للتحديث";
    tray_update_v: "Restart to update ({})"
        | "أعد التشغيل للتحديث ({})";
    tray_get_v: "Get OpenCircle {}"
        | "احصل على OpenCircle {}";
    tray_tip: "OpenCircle · {}"
        | "OpenCircle · {}";
    tray_save: "Save…"
        | "حفظ…";
    up_out: "OpenCircle {} is out"
        | "صدر OpenCircle {}";
    up_ready: "OpenCircle {} is ready"
        | "OpenCircle {} جاهز";
    up_updating: "Updating OpenCircle…"
        | "جارٍ تحديث OpenCircle…";
    up_updated: "Updated to {}"
        | "تم التحديث إلى {}";
    up_line_restart: "Installs when you restart OpenCircle"
        | "يُثبَّت عند إعادة تشغيل OpenCircle";
    up_line_setup: "Windows asks once for permission to install"
        | "سيطلب Windows إذنًا مرة واحدة للتثبيت";
    up_line_getit: "Update it with your package manager or the release page"
        | "حدّثه عبر مدير الحزم أو صفحة الإصدار";
    up_line_back: "Back in a few seconds"
        | "نعود خلال ثوانٍ";
    up_later: "Later"
        | "لاحقًا";
    wn_title: "What's new in OpenCircle {}"
        | "ما الجديد في OpenCircle {}";
    wn_installed: "Updated just now"
        | "تم التحديث للتو";
    wn_highlights: "Highlights"
        | "أبرز التغييرات";
    wn_more: "{} more in the full release notes"
        | "{} أخرى في ملاحظات الإصدار الكاملة";
    wn_fixes: "Fixes"
        | "الإصلاحات";
    wn_upgrade: "Upgrade notes"
        | "ملاحظات الترقية";
    wn_done: "Done"
        | "تم";
    wn_full: "Full release notes"
        | "ملاحظات الإصدار الكاملة";
    pill_copied: "Copied"
        | "تم النسخ";
    pill_copy_failed: "Couldn't copy. Close what holds the clipboard and try again."
        | "تعذّر النسخ. أغلق ما يستخدم الحافظة وحاول مرة أخرى.";
    pill_cant_copy: "Couldn't copy"
        | "تعذّر النسخ";
    pill_saved: "Saved to {}"
        | "تم الحفظ في {}";
    pill_not_saved: "Couldn't save. Pick a folder you can write to in Settings."
        | "تعذّر الحفظ. اختر في الإعدادات مجلدًا يمكنك الكتابة فيه.";
    pill_no_browser: "Couldn't open your browser. Check that a default browser is set, then try again."
        | "تعذّر فتح المتصفح. تأكد من تعيين متصفح افتراضي ثم حاول مرة أخرى.";
    pill_diag: "Diagnostics copied"
        | "تم نسخ بيانات التشخيص";
    pill_pin_gone: "The pinned window closed"
        | "أُغلقت النافذة المثبّتة";
    pill_pin_still: "Can't follow this window: the pin stays still"
        | "تعذّرت متابعة هذه النافذة: يبقى التثبيت ثابتًا";
    pill_pins_max: "Up to 4 live pins: close one first"
        | "حتى 4 تثبيتات حيّة: أغلق أحدها أولًا";
    pill_pins_pick: "Pick a part of one window to pin it live"
        | "اختر جزءًا من نافذة واحدة لتثبيته حيًّا";
    pill_tray: "OpenCircle is running in the tray. Press {}."
        | "يعمل OpenCircle في علبة النظام. اضغط {}.";
    pill_taken_one: "{} is taken by another app: pick another key in Settings"
        | "{} مستخدم في تطبيق آخر: اختر مفتاحًا آخر في الإعدادات";
    pill_taken_many: "{} are taken by another app: pick another key in Settings"
        | "{} مستخدمة في تطبيق آخر: اختر مفاتيح أخرى في الإعدادات";
    md_header: "On-device models"
        | "النماذج على الجهاز";
    md_ocr_name: "Text recognition"
        | "التعرّف على النص";
    md_ocr_title: "Text recognition needs a small download"
        | "يحتاج التعرّف على النص إلى تنزيل صغير";
    md_ocr_does: "Read text in images"
        | "قراءة النص في الصور";
    md_scripts_name: "Text in other scripts"
        | "النص بالخطوط الأخرى";
    md_scripts_title: "This text needs another reader"
        | "يحتاج هذا النص إلى قارئ آخر";
    md_scripts_does: "Read Arabic, Cyrillic, Chinese and Japanese text"
        | "قراءة النص العربي والسيريلي والصيني والياباني";
    md_tr_name: "Local translation"
        | "الترجمة على الجهاز";
    md_tr_title: "Local translation needs a download"
        | "تحتاج الترجمة على الجهاز إلى تنزيل";
    md_tr_does: "Translate without internet"
        | "الترجمة دون إنترنت";
    md_meta: "{} · {} · from {}"
        | "{} · {} · من {}";
    md_downloading: "Downloading {}"
        | "جارٍ تنزيل {}";
    md_checking: "Checking your download"
        | "جارٍ فحص التنزيل";
    md_failed: "Download could not finish"
        | "تعذّر إكمال التنزيل";
    md_on_pc: "{} on this PC; none of your content is sent."
        | "{} على هذا الجهاز؛ ولا يُرسل أي من محتواك.";
    md_download: "Download"
        | "تنزيل";
    md_not_now: "Not now"
        | "ليس الآن";
    md_provider: "Use an AI provider instead"
        | "استخدام مزوّد ذكاء اصطناعي بدلًا من ذلك";
    md_retrying: "Retrying…"
        | "جارٍ إعادة المحاولة…";
    md_progress: "{}"
        | "{}";
    md_verifying: "Checking integrity and setting up on this PC…"
        | "جارٍ فحص السلامة والإعداد على هذا الجهاز…";
    md_try_again: "Try again"
        | "حاول مرة أخرى";
    md_err_server: "The download server had a problem. Try again in a minute."
        | "واجه خادم التنزيل مشكلة. حاول مرة أخرى بعد دقيقة.";
    md_err_offline: "Could not reach the download server. Check your internet connection, then try again in a minute."
        | "تعذّر الوصول إلى خادم التنزيل. تحقق من اتصالك بالإنترنت ثم حاول مرة أخرى بعد دقيقة.";
    md_err_damaged: "The download was damaged. Try again."
        | "التنزيل تالف. حاول مرة أخرى.";
    md_err_other: "The download could not finish. Try again."
        | "تعذّر إكمال التنزيل. حاول مرة أخرى.";
    md_read_names: "Read {} text"
        | "قراءة نص {}";
    tp_found: "{} text found"
        | "عُثر على نص بلغة {}";
    tp_get: "Get {} → {}"
        | "تنزيل {} ← {}";
    tp_translating: "Translating…"
        | "جارٍ الترجمة…";
    tp_set_up_ai: "Set up AI"
        | "إعداد الذكاء الاصطناعي";
    tp_thinking: "Thinking…"
        | "جارٍ التفكير…";
    tp_explain: "Explain"
        | "اشرح";
    pk_suggested: "Suggested"
        | "مقترحة";
    pk_recent: "Recent"
        | "الأخيرة";
    pk_all: "All languages"
        | "كل اللغات";
    pk_detect_long: "Detect language"
        | "اكتشاف اللغة";
    pk_detect: "Detect"
        | "اكتشاف";
    pk_search: "Search languages"
        | "ابحث عن لغة";
    pk_none: "No language found"
        | "لا توجد لغة";
    pk_needs_ai: "Needs AI · no local model yet"
        | "يحتاج الذكاء الاصطناعي · لا نموذج دون اتصال بعد";
}
