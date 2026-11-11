# Original route-consumer check receipts

AC-T7.4.1. Data-only collection from three closed ROOT cohorts; all 27 archive members were independently checked against their byte lengths and SHA256, and source manifests match each pinned source archive. The recorded CLI 98790 exited 0 and was reaped. No tests or native effects were invoked by collection. Raw logs remain lossless inside raw.tar.gz.

- 378192b7: pool record 4 passed; wire controls 13 passed and 2 failed because their positive fixtures used 4KiB below the existing 1MiB API minimum.
- a39b9dd5: the two corrected wire controls and new BackingUsage client index passed. Production compilation failed at two Mac watcher borrows; these were subsequently corrected in ece9d243 using the owned d849 source patch.
- 4f0922ae: route-index and peer test compilation and production checking each exited 101 on aggregate Clone derives over the now-fallible FitIndex. Zero intended controls executed. This is not evidence of the three intended route/grant failures. All recorded children were reaped; temporary directories were empty and source manifests stayed unchanged except the explicitly recorded private vendor lock.

The archive is 654362 bytes, SHA256 e665c42537ddf5799ce1870fd8a94262672ad539b7852af961b0244958f96de8. Executable hashes are recorded separately per cohort; their presence alone does not establish that later controls executed. No backend execution, GPU isolation, recovery, performance or completion claim follows.
