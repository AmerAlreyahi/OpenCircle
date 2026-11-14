//! Common game and settings menu labels, our own list: offline MT reads a
//! label alone in its everyday sense ("Controles" -> "Checks", "Brillo" ->
//! "Shine"), and into Arabic through English made it worse. A label that is
//! exactly one of these words is translated from this table; anything else
//! goes to the model. Nouns and verbs only: an adjective ("Alta", "Activée")
//! takes its noun's gender, which the model sees and a table doesn't.

use crate::features::translate::lang;

/// Languages of the columns, in order.
const COLUMNS: [&str; 7] = ["en", "es", "fr", "de", "it", "pt", "ar"];

/// One label per row, in each language (`/` = other spellings; the first is
/// written).
const ROWS: &[[&str; 7]] = &[
    [
        "Settings",
        "Ajustes/Configuración",
        "Paramètres/Réglages",
        "Einstellungen",
        "Impostazioni",
        "Configurações/Definições",
        "الإعدادات",
    ],
    [
        "Options",
        "Opciones",
        "Options",
        "Optionen",
        "Opzioni",
        "Opções",
        "الخيارات",
    ],
    [
        "Graphics",
        "Gráficos",
        "Graphismes/Graphiques",
        "Grafik",
        "Grafica",
        "Gráficos",
        "الرسومات",
    ],
    [
        "Audio",
        "Audio",
        "Audio",
        "Audio",
        "Audio",
        "Áudio",
        "الصوت",
    ],
    ["Sound", "Sonido", "Son", "Ton", "Suono", "Som", "الصوت"],
    [
        "Controls",
        "Controles",
        "Commandes/Contrôles",
        "Steuerung",
        "Comandi/Controlli",
        "Controles/Controlos",
        "التحكم",
    ],
    [
        "Language",
        "Idioma",
        "Langue",
        "Sprache",
        "Lingua",
        "Idioma",
        "اللغة",
    ],
    [
        "Account",
        "Cuenta",
        "Compte",
        "Konto",
        "Account",
        "Conta",
        "الحساب",
    ],
    [
        "Brightness",
        "Brillo",
        "Luminosité",
        "Helligkeit",
        "Luminosità",
        "Brilho",
        "السطوع",
    ],
    [
        "Contrast",
        "Contraste",
        "Contraste",
        "Kontrast",
        "Contrasto",
        "Contraste",
        "التباين",
    ],
    [
        "Volume",
        "Volumen",
        "Volume",
        "Lautstärke",
        "Volume",
        "Volume",
        "مستوى الصوت",
    ],
    [
        "Music",
        "Música",
        "Musique",
        "Musik",
        "Musica",
        "Música",
        "الموسيقى",
    ],
    [
        "Subtitles",
        "Subtítulos",
        "Sous-titres",
        "Untertitel",
        "Sottotitoli",
        "Legendas",
        "الترجمة",
    ],
    [
        "Resolution",
        "Resolución",
        "Résolution",
        "Auflösung",
        "Risoluzione",
        "Resolução",
        "الدقة",
    ],
    [
        "Fullscreen/Full screen",
        "Pantalla completa",
        "Plein écran",
        "Vollbild",
        "Schermo intero",
        "Tela cheia/Ecrã inteiro",
        "ملء الشاشة",
    ],
    [
        "Shadows",
        "Sombras",
        "Ombres",
        "Schatten",
        "Ombre",
        "Sombras",
        "الظلال",
    ],
    [
        "Apply",
        "Aplicar",
        "Appliquer",
        "Übernehmen/Anwenden",
        "Applica",
        "Aplicar",
        "تطبيق",
    ],
    [
        "Reset",
        "Restablecer/Restaurar",
        "Réinitialiser",
        "Zurücksetzen",
        "Ripristina",
        "Redefinir/Repor",
        "إعادة تعيين",
    ],
    [
        "Cancel",
        "Cancelar",
        "Annuler",
        "Abbrechen",
        "Annulla",
        "Cancelar",
        "إلغاء",
    ],
    [
        "Back",
        "Volver/Atrás",
        "Retour",
        "Zurück",
        "Indietro",
        "Voltar",
        "رجوع",
    ],
    [
        "Confirm",
        "Confirmar",
        "Confirmer",
        "Bestätigen",
        "Conferma",
        "Confirmar",
        "تأكيد",
    ],
    [
        "Save",
        "Guardar",
        "Sauvegarder/Enregistrer",
        "Speichern",
        "Salva",
        "Salvar/Guardar",
        "حفظ",
    ],
    [
        "Load",
        "Cargar",
        "Charger",
        "Laden",
        "Carica",
        "Carregar",
        "تحميل",
    ],
    [
        "Continue",
        "Continuar",
        "Continuer",
        "Weiter",
        "Continua",
        "Continuar",
        "متابعة",
    ],
    [
        "New Game",
        "Nueva partida",
        "Nouvelle partie",
        "Neues Spiel",
        "Nuova partita",
        "Novo jogo",
        "لعبة جديدة",
    ],
    [
        "Quit/Exit",
        "Salir",
        "Quitter",
        "Beenden",
        "Esci",
        "Sair",
        "خروج",
    ],
    [
        "Inventory",
        "Inventario",
        "Inventaire",
        "Inventar",
        "Inventario",
        "Inventário",
        "المخزون",
    ],
    ["Map", "Mapa", "Carte", "Karte", "Mappa", "Mapa", "الخريطة"],
    [
        "Quests",
        "Misiones",
        "Quêtes",
        "Aufträge",
        "Missioni",
        "Missões",
        "المهام",
    ],
    [
        "Skills",
        "Habilidades",
        "Compétences",
        "Fähigkeiten",
        "Abilità",
        "Habilidades",
        "المهارات",
    ],
    [
        "Difficulty",
        "Dificultad",
        "Difficulté",
        "Schwierigkeit",
        "Difficoltà",
        "Dificuldade",
        "الصعوبة",
    ],
    [
        "Pause",
        "Pausa",
        "Pause",
        "Pause",
        "Pausa",
        "Pausa",
        "إيقاف مؤقت",
    ],
    [
        "Resume",
        "Reanudar",
        "Reprendre",
        "Fortsetzen",
        "Riprendi",
        "Retomar",
        "استئناف",
    ],
    [
        "Keyboard",
        "Teclado",
        "Clavier",
        "Tastatur",
        "Tastiera",
        "Teclado",
        "لوحة المفاتيح",
    ],
    [
        "Shop",
        "Tienda",
        "Boutique",
        "Shop",
        "Negozio",
        "Loja",
        "المتجر",
    ],
    [
        "Summon",
        "Invocar",
        "Invoquer",
        "Beschwören",
        "Evoca",
        "Invocar",
        "استدعاء",
    ],
    // an inventory's tab and an item's stat (de-en alone: "Watering",
    // "Shelf life"; the corpus' inventory)
    [
        "Potions",
        "Pociones",
        "Potions",
        "Tränke",
        "Pozioni",
        "Poções",
        "الجرعات",
    ],
    [
        "Durability",
        "Durabilidad",
        "Durabilité",
        "Haltbarkeit",
        "Durabilità",
        "Durabilidade",
        "المتانة",
    ],
];

/// `label`'s translation into `target` when it is exactly one of the
/// table's labels (accents, case and a trailing ":" aside) in another
/// language, else `None`. The target's own word means nothing to redraw.
pub fn lookup(label: &str, target: &str) -> Option<&'static str> {
    let to = COLUMNS
        .iter()
        .position(|c| lang::find(target).is_some_and(|l| l.code == *c))?;
    let key = |s: &str| -> String {
        (s.trim().trim_end_matches(':').chars())
            .map(lang::plain)
            .collect::<String>()
    };
    let label = key(label);
    ROWS.iter().find_map(|row| {
        let hit = (row.iter()).any(|cell| cell.split('/').any(|w| key(w) == label));
        hit.then(|| row[to].split('/').next().unwrap_or(row[to]))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_words_take_their_menu_sense() {
        // offline MT alone: "Checks", "Shine" (Win10 VM, a Spanish settings menu)
        assert_eq!(lookup("Controles", "en"), Some("Controls"));
        assert_eq!(lookup("Brillo", "en"), Some("Brightness"));
        assert_eq!(lookup("Gráficos", "ar"), Some("الرسومات"));
        assert_eq!(lookup("Restablecer", "fr"), Some("Réinitialiser"));
        assert_eq!(lookup("Aplicar", "de"), Some("Übernehmen"));
        assert_eq!(lookup("LANGUE:", "es"), Some("Idioma"));
        assert_eq!(lookup("Tränke", "en"), Some("Potions"));
        assert_eq!(lookup("Haltbarkeit", "ar"), Some("المتانة"));
        // a phrase, a word not in the table, a target not in it
        assert_eq!(lookup("Brillo de pantalla", "en"), None);
        assert_eq!(lookup("Alta", "en"), None);
        assert_eq!(lookup("Brillo", "ru"), None);
        for row in ROWS {
            assert!(row.iter().all(|c| !c.is_empty() && !c.contains("//")));
        }
    }
}
