export default {
  extends: ['stylelint-config-standard', 'stylelint-config-html/svelte'],
  rules: {
    'color-no-hex': true,
    // Raw colour literals are the drift @poodle64/design-tokens exists to stop:
    // a value written at one call site is invisible to every theme, accent and
    // contrast change afterwards (#226 was exactly that, in token form). Bans a
    // literal channel value in ANY notation — `rgb(255,255,255,.06)`,
    // `oklch(0.55 0.12 30)` — while leaving `rgb(var(--p11-overlay-rgb) / 0.06)`
    // alone: the CHANNELS must come from a token, the alpha stays at the call
    // site where that emphasis decision belongs. A generated ramp
    // (`oklch(0.55 0.12 var(--tile-hue))`) is a token in tokens.css, not a
    // component literal, so it is covered by the override below rather than an
    // exemption in the pattern — a notation-shaped hole is how this rule would
    // otherwise erode.
    'declaration-property-value-disallowed-list': [
      {
        '/.*/': [
          '/(rgba?|hsla?|oklch|oklab|lab|lch|hwb|color)\\(\\s*[0-9.]/',
          '/\\b(white|black|red|lime|blue|yellow|magenta|cyan|silver|gray|grey|maroon|olive|green|purple|teal|navy|fuchsia|aqua)\\b/',
        ],
        // Same drift as a raw colour literal, for type: a fixed px value at
        // one call site is invisible to a scale change everywhere else,
        // which is exactly how the same "sort indicator" glyph ended up at
        // three different sizes (8px/9px/10px) across two views. A fluid
        // `clamp(...)` headline is left alone — that is a deliberate
        // responsive display size, not a drifted scale step.
        'font-size': ['/^[0-9]/'],
      },
      {
        message: (name) =>
          name === 'font-size' ? `Raw type size in "font-size" — use a --font-size-* token.` : `Raw colour literal in "${name}" — use a --p11-* token, or rgb(var(--p11-overlay-rgb) / <alpha>).`,
      },
    ],
    // Notation preferences — disabled to avoid fighting existing formatting
    'at-rule-no-unknown': null,
    // `@apply` takes a list of Tailwind utilities, not a CSS value.
    'at-rule-prelude-no-invalid': [true, { ignoreAtRules: ['apply'] }],
    'custom-property-pattern': null,
    'selector-class-pattern': null,
    'no-descending-specificity': null,
    'import-notation': null,
    'comment-empty-line-before': null,
    'declaration-empty-line-before': null,
    'value-keyword-case': null,
    'lightness-notation': null,
    'hue-degree-notation': null,
    // Modern colour notation enforcement — disabled; rgba() is in existing tokens
    'color-function-notation': null,
    'color-function-alias-notation': null,
    'alpha-value-notation': null,
    // Formatting rules that fight Tailwind/existing structure
    'rule-empty-line-before': null,
    'at-rule-empty-line-before': null,
    'custom-property-empty-line-before': null,
    'declaration-block-single-line-max-declarations': null,
    // Svelte-specific: :global() is a valid Svelte pseudo-class
    'selector-pseudo-class-no-unknown': null,
    // Svelte inline style="..." attributes are declaration-only blocks with no
    // wrapping rule, which this rule mis-reports as invalid-position. Disable it.
    'no-invalid-position-declaration': null,
    // CSS nesting — @custom-variant uses top-level & nesting
    'nesting-selector-no-missing-scoping-root': null,
    // Vendor prefixes in existing code
    'property-no-vendor-prefix': null,
    // Misc formatting
    'length-zero-no-unit': null,
    'media-feature-range-notation': null,
    'declaration-block-no-redundant-longhand-properties': null,
    'keyframes-name-pattern': null,
  },
  overrides: [
    {
      // The two palette files are where a colour literal is SUPPOSED to live:
      // src/lib/styles/tokens.css defines the P11 tokens, src/app.css the
      // shadcn ones. Everything else consumes them.
      files: ['src/lib/styles/tokens.css', 'src/app.css'],
      rules: {
        'declaration-property-value-disallowed-list': null,
      },
    },
  ],
};
