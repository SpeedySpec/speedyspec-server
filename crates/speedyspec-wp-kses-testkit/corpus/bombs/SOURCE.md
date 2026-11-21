# Expansion bombs

Written for the kses port (no upstream source). XML entity bombs (billion laughs, quadratic blowup, parameter
entities), external references (XXE with SYSTEM and PUBLIC, file/http/php/expect/data URIs, XInclude, external DTDs,
processing instructions, ATTLIST/ELEMENT declarations), SVG reference bombs (exponential and recursive `use`,
`pattern`, `filter`, `marker`), SVG `animate`/`set` rewriting `href`, and DOCTYPE and CDATA tricks in posts. The bombs
aimed at kses itself are generators in `src/bombs.rs`, sized per test.
