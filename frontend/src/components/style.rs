// All CSS constants use CLASS_ prefix for consistency.

// --- Layout ---
pub const CLASS_LABEL: &str = "block text-sm font-medium text-gray-700 dark:text-ink-300 mb-1";
pub const CLASS_INPUT: &str = "w-full px-3 py-2 border border-gray-300 dark:border-ink-600 \
    rounded-lg bg-white dark:bg-ink-800 text-gray-900 dark:text-ink-100 \
    focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none";
pub const CLASS_ICON_BTN: &str = "cursor-pointer text-gray-400 hover:text-gray-600 \
    dark:hover:text-ink-300 transition-colors active:scale-95";

// --- Buttons ---
pub const CLASS_BTN_CANCEL: &str = "px-4 py-2 text-sm text-gray-600 dark:text-ink-400 \
    hover:text-gray-800 dark:hover:text-ink-200 cursor-pointer transition-colors active:scale-95";

pub const CLASS_HERO_BTN: &str = "px-6 py-3 bg-indigo-600 hover:bg-indigo-700 text-white \
    font-semibold rounded-lg cursor-pointer transition-colors inline-block active:scale-95";

pub const CLASS_BTN_PRIMARY: &str = "px-4 py-2 bg-blue-500 hover:enabled:bg-blue-600 text-white \
    text-sm font-medium rounded-lg transition-colors disabled:opacity-50 \
    disabled:cursor-not-allowed flex items-center gap-2 cursor-pointer active:scale-95";

pub const CLASS_BTN_DANGER: &str = "px-4 py-2 bg-red-500 hover:enabled:bg-red-600 text-white \
    text-sm font-medium rounded-lg transition-colors disabled:opacity-50 \
    disabled:cursor-not-allowed flex items-center gap-2 cursor-pointer active:scale-95";

// --- Page ---
pub const CLASS_PAGE_TITLE: &str = "text-2xl font-bold text-gray-900 dark:text-ink-100 mb-6";
pub const CLASS_NAV_LINK: &str = "flex items-center gap-2 px-3 py-2 cursor-pointer transition-colors \
    font-semibold text-gray-600 dark:text-ink-300 hover:text-indigo-600 dark:hover:text-indigo-400 \
    aria-[current=page]:text-indigo-600 dark:aria-[current=page]:text-indigo-400";
// --- Detail modal rows ---
pub const CLASS_DETAIL_DIVIDER: &str = "space-y-0 divide-y divide-gray-100 dark:divide-ink-700";
pub const CLASS_DETAIL_LABEL: &str = "text-sm text-gray-500 dark:text-ink-400";
pub const CLASS_DETAIL_VALUE: &str =
    "text-sm text-gray-900 dark:text-ink-100 font-medium text-right ml-4 truncate max-w-[60%]";
pub const CLASS_DETAIL_VALUE_PLAIN: &str =
    "text-sm text-gray-900 dark:text-ink-100 font-medium text-right ml-4";
pub const CLASS_DETAIL_VALUE_MONO: &str = "text-sm text-gray-900 dark:text-ink-100 font-medium text-right ml-4 truncate max-w-[60%] font-mono";
pub const CLASS_DETAIL_VALUE_TAG: &str = "text-sm text-right ml-4";

// --- Form ---
pub const CLASS_FORM_FOOTER: &str = "flex items-center justify-end gap-3";
pub const CLASS_TOGGLE_LABEL: &str = "text-sm text-gray-700 dark:text-ink-300";
pub const CLASS_TEXT_MUTED: &str = "text-gray-500 dark:text-ink-400";
pub const CLASS_CARD: &str = "bg-white dark:bg-ink-900 rounded-xl shadow-sm";
pub const CLASS_BORDER_B: &str = "border-b border-gray-100 dark:border-ink-700";
pub const CLASS_BG_MUTED: &str = "bg-gray-100 dark:bg-ink-800";
pub const CLASS_PILL_GREEN: &str =
    "bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-400";
pub const CLASS_PILL_RED: &str = "bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-400";
pub const CLASS_FORM_ERROR: &str = "text-red-500 text-sm";
pub const CLASS_DISABLED_INPUT: &str = "w-full px-3 py-2 border border-gray-300 \
    dark:border-ink-600 rounded-lg bg-gray-100 dark:bg-ink-700 \
    text-gray-900 dark:text-ink-100 cursor-not-allowed";
