/**
 * Text from the argument, made safe to put on screen.
 *
 * Control characters are dropped, and so are the bidirectional embedding,
 * override and isolate controls, so text cannot be shown in an order other
 * than the one that was hashed. Tabs and line breaks are ordinary content and
 * stay, as do other format characters such as the joiners emoji are built
 * from. This is what `inka review` does before it prints (`sanitize` in
 * `crate/inka-cli/src/review.rs`); the two must agree on what a reader sees.
 */

const DROPPED = /[\p{Cc}‪-‮⁦-⁩]/gu

export function neutralize(text: string): string {
  return text.replace(DROPPED, (character) => (character === '\t' || character === '\n' ? character : ''))
}
