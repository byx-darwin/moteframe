# Configuration registry implementation plan

Authorized scope: only this independent workspace. Keep wire1.2 and every V0.5 complete entry byte-for-byte. V0.6 layouts are design candidates, not individually frozen product approval.

1. Extract exact typed layouts and numeric dictionaries. Explicitly distinguish schema validation from actual backend capability.
2. RED literal/extra/mask/enumeration/product-boundary tests; implement typed DTO decode+encode and feature-gated registry.
3. Delegate stable profile/reminder/sleep/women/UV codecs to their owning crates. Do not duplicate those validators.
4. Cover all62 properties and15 objects in an ownership ledger. External-content routes require an explicitly supplied SchemaProvider; no permissive raw fallback.
5. Verify default/all/single features on Host and thumbv6m, lint both targets, record exact commands and results. Freeze for root's independent final review.

The production registry has no state store, allocation, sensor algorithm, hardware action, authorization authority or GATT integration. Engineering guard and current cross-policy state are caller-supplied facts.
