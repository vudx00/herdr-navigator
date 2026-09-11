# History fixtures

`provider-pins-v1.json` is a storage-local preference document. The generated `provider_owner` distinguishes installations, not machines or clients. Copying this file copies preferences and their owner namespace. It grants no right to execute an action.

`legacy-cases.json` covers valid-looking, ambiguous, unknown, repeated, malformed and incomplete legacy pin lists. Every imported value stays unresolved. No number of matching fresh rows proves an owner. Removed unresolved records retain exact source/value provenance as tombstones. Old files remain byte-for-byte unchanged.

`qualified-history-cases.json` records the missing replay evidence and future acceptance cases. It is a blocked contract checklist, not a synthetic transport or an implemented client-history test suite. Existing domain tests check live ancestry and scope separation. The current dispatch result distinguishes exact ordinary focus from provider completion. Neither carries client-transition authorization, and neither writes workspace history.

Storage tests use injected temporary directories, never user state. Limits are 1 MiB per input/output document, 4096 records including tombstones, and 16 KiB per string. Reads are bounded before parsing. No eviction occurs. A stable advisory lock protects reload/change/replace transactions. A unique mode-0600 sibling file is synced before rename, then the directory is synced. Failure after rename reports uncertain durability and disables further writes on that store instance.
