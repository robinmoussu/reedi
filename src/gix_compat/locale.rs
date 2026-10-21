//! The locale of the C library, used like git does: for `strftime`, and to translate messages
//! with the translations of git (gettext).

use std::ffi::CStr;
use std::ffi::CString;
use std::ffi::c_char;
use std::ffi::c_int;
use std::ffi::c_ulong;
use std::ptr;

use jiff::Zoned;

/// The text domain of the translations of git.
const GIT_TEXT_DOMAIN: &CStr = c"git";

/// Where the translations of git are installed, unless `GIT_TEXTDOMAINDIR` is set.
const DEFAULT_TRANSLATIONS_DIRECTORY: &str = "/usr/share/locale";

/// The charset of the translations, whatever the one of the locale is.
const TRANSLATIONS_CHARSET: &CStr = c"UTF-8";

/// The placeholder of the number in the messages of git.
const COUNT_PLACEHOLDER: &str = "%lu";

/// The biggest result of `strftime` that is supported.
const MAX_STRFTIME_LEN: usize = 1 << 16;

unsafe extern "C" {
    fn dcgettext(domain: *const c_char, message: *const c_char, category: c_int) -> *mut c_char;
    fn dcngettext(
        domain: *const c_char,
        singular: *const c_char,
        plural: *const c_char,
        count: c_ulong,
        category: c_int,
    ) -> *mut c_char;
    fn bindtextdomain(domain: *const c_char, directory: *const c_char) -> *mut c_char;
    fn bind_textdomain_codeset(domain: *const c_char, codeset: *const c_char) -> *mut c_char;
}

/// A locale of the C library. It is only used by the thread using it (`uselocale`), so that
/// several locales can be used at the same time (like in the tests).
#[derive(Debug)]
pub struct Locale {
    raw: libc::locale_t,
}

impl Drop for Locale {
    fn drop(&mut self) {
        // SAFETY: `raw` was created by `newlocale` and is freed only here.
        unsafe { libc::freelocale(self.raw) };
    }
}

/// `pattern` where `%z` and `%Z` are replaced like git does before calling `strftime`: the C
/// library would use the timezone of the process instead of the one of `zoned` for `%Z`.
fn expand_timezone(pattern: &str, zoned: &Zoned, local: bool) -> String {
    let mut expanded = String::new();
    let mut chars = pattern.chars();
    while let Some(char) = chars.next() {
        if char != '%' {
            expanded.push(char);
            continue;
        }
        match chars.next() {
            Some('z') => {
                let seconds = zoned.offset().seconds();
                let minutes = seconds.abs() / 60;
                let sign = if seconds < 0 { '-' } else { '+' };
                expanded += &format!("{sign}{:02}{:02}", minutes / 60, minutes % 60);
            }
            Some('Z') => {
                if local {
                    let info = zoned.time_zone().to_offset_info(zoned.timestamp());
                    expanded += &info.abbreviation().replace('%', "%%");
                }
            }
            Some(other) => {
                expanded.push('%');
                expanded.push(other);
            }
            None => expanded.push('%'),
        }
    }
    expanded
}

impl Locale {
    fn new(name: &CStr) -> Option<Self> {
        // SAFETY: `name` is a valid C string, and there is no base locale.
        let raw = unsafe { libc::newlocale(libc::LC_ALL_MASK, name.as_ptr(), ptr::null_mut()) };
        if raw.is_null() {
            None
        } else {
            Some(Self { raw })
        }
    }

    /// The `C` locale: no translation, English names.
    pub fn c() -> Self {
        Self::new(c"C").expect("the C locale always exists")
    }

    /// The locale of the environment (`LC_ALL`, `LC_TIME`, `LC_MESSAGES`, `LANG`), like git.
    pub fn from_environment() -> Self {
        Self::new(c"").unwrap_or_else(Self::c)
    }

    /// The locale named `name` (like `fr_FR.UTF-8`), or `None` if it is not installed.
    pub fn named(name: &str) -> Option<Self> {
        Self::new(&CString::new(name).ok()?)
    }

    /// `time` (in the timezone of `zoned`) formatted with the `strftime` of the C library, like
    /// `git log --date=format:$pattern`: `%z` is the offset, and `%Z` the name of the timezone
    /// only if `local` (it is empty otherwise).
    pub fn strftime(&self, pattern: &str, zoned: &Zoned, local: bool) -> String {
        let Ok(pattern) = CString::new(expand_timezone(pattern, zoned, local)) else {
            return String::new();
        };
        // SAFETY: `tm` is a plain C struct, for which all zeros is valid.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        tm.tm_year = c_int::from(zoned.year()) - 1900;
        tm.tm_mon = c_int::from(zoned.month()) - 1;
        tm.tm_mday = c_int::from(zoned.day());
        tm.tm_hour = c_int::from(zoned.hour());
        tm.tm_min = c_int::from(zoned.minute());
        tm.tm_sec = c_int::from(zoned.second());
        tm.tm_wday = c_int::from(zoned.weekday().to_sunday_zero_offset());
        tm.tm_yday = c_int::from(zoned.day_of_year()) - 1;
        tm.tm_isdst = c_int::from(
            zoned
                .time_zone()
                .to_offset_info(zoned.timestamp())
                .dst()
                .is_dst(),
        );
        tm.tm_gmtoff = zoned.offset().seconds().into();
        // `strftime` returns 0 both when the result is empty and when the buffer is too small.
        std::iter::successors(Some(256), |len| {
            Some(len * 2).filter(|len| *len <= MAX_STRFTIME_LEN)
        })
        .find_map(|len| {
            let mut buffer = vec![0u8; len];
            // SAFETY: `buffer` is `len` bytes long, and the other pointers are valid C
            // strings and structs which outlive the call.
            let written = unsafe {
                libc::strftime_l(
                    buffer.as_mut_ptr().cast(),
                    len,
                    pattern.as_ptr(),
                    &tm,
                    self.raw,
                )
            };
            buffer.truncate(written);
            if written > 0 {
                Some(String::from_utf8_lossy(&buffer).into_owned())
            } else {
                None
            }
        })
        .unwrap_or_default()
    }

    /// The translation by git of `singular` or `plural`, depending on `count` (`Q_()` in git),
    /// with `count` replacing `%lu`.
    pub fn translate_plural(&self, singular: &str, plural: &str, count: u64) -> String {
        let (Ok(singular), Ok(plural)) = (CString::new(singular), CString::new(plural)) else {
            return String::new();
        };
        let translated = self.with_translations(|| {
            // SAFETY: the arguments are valid C strings, and the result is either one of them or
            // a string owned by gettext, valid until the next call.
            unsafe {
                let translated = dcngettext(
                    GIT_TEXT_DOMAIN.as_ptr(),
                    singular.as_ptr(),
                    plural.as_ptr(),
                    count,
                    libc::LC_MESSAGES,
                );
                CStr::from_ptr(translated).to_string_lossy().into_owned()
            }
        });
        translated.replacen(COUNT_PLACEHOLDER, &count.to_string(), 1)
    }

    /// The translation by git of `message` (`_()` in git).
    pub fn translate(&self, message: &str) -> String {
        let Ok(message) = CString::new(message) else {
            return String::new();
        };
        self.with_translations(|| {
            // SAFETY: same as in `translate_plural`.
            unsafe {
                let translated = dcgettext(
                    GIT_TEXT_DOMAIN.as_ptr(),
                    message.as_ptr(),
                    libc::LC_MESSAGES,
                );
                CStr::from_ptr(translated).to_string_lossy().into_owned()
            }
        })
    }

    /// Runs `translate` with this locale as the locale of the thread, and the translations of git
    /// installed.
    fn with_translations<T>(&self, translate: impl FnOnce() -> T) -> T {
        let directory = std::env::var("GIT_TEXTDOMAINDIR")
            .unwrap_or_else(|_| DEFAULT_TRANSLATIONS_DIRECTORY.to_string());
        let directory = CString::new(directory).unwrap_or_default();
        // SAFETY: the arguments are valid C strings, which gettext copies.
        unsafe {
            bindtextdomain(GIT_TEXT_DOMAIN.as_ptr(), directory.as_ptr());
            bind_textdomain_codeset(GIT_TEXT_DOMAIN.as_ptr(), TRANSLATIONS_CHARSET.as_ptr());
        }
        // SAFETY: `raw` is a valid locale, and the previous one is restored below.
        let previous = unsafe { libc::uselocale(self.raw) };
        let translated = translate();
        // SAFETY: `previous` is the locale that was in use by the thread.
        unsafe { libc::uselocale(previous) };
        translated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_locale_is_none() {
        assert!(Locale::named("xx_XX.UTF-8").is_none());
    }

    #[test]
    fn c_locale_does_not_translate() {
        let locale = Locale::c();
        assert_eq!(locale.translate("in the future"), "in the future");
        assert_eq!(
            locale.translate_plural("%lu hour ago", "%lu hours ago", 1),
            "1 hour ago"
        );
        assert_eq!(
            locale.translate_plural("%lu hour ago", "%lu hours ago", 3),
            "3 hours ago"
        );
    }
}
