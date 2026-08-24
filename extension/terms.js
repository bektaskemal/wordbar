/* terms.js — pure helpers, no GI imports.
 *
 * Kept free of gi:// so it can be unit-tested with plain gjs, and so the
 * capture and panel code share one definition of what a term looks like.
 */

/** Mirrors the service's validation; see wordbar-core::normalize. */
export const MAX_TERM_CHARS = 80;
export const MAX_TERM_WORDS = 5;

/**
 * Cheap client-side plausibility check.
 *
 * The service normalizes and validates authoritatively; this only decides
 * between "save silently" and "ask first", so it errs toward asking.
 *
 * @param {?string} text
 * @returns {boolean}
 */
export function looksLikeTerm(text) {
    if (!text)
        return false;
    const trimmed = text.trim();
    if (trimmed === '' || trimmed.length > MAX_TERM_CHARS)
        return false;
    if (trimmed.includes('\n'))
        return false;
    if (trimmed.split(/\s+/).length > MAX_TERM_WORDS)
        return false;
    // Needs at least one letter; URLs and code fragments are not vocabulary.
    if (!/\p{L}/u.test(trimmed))
        return false;
    if (/^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed))
        return false;
    return true;
}

/**
 * Shorten a term for the top bar so the panel layout stays stable.
 *
 * The result never exceeds `maxChars`, ellipsis included.
 *
 * @param {string} text
 * @param {number} maxChars
 * @returns {string}
 */
export function ellipsize(text, maxChars) {
    const characters = [...text];
    if (characters.length <= maxChars)
        return text;
    if (maxChars <= 1)
        return '…';
    return `${characters.slice(0, maxChars - 1).join('')}…`;
}
