# Sample screens

`make_samples.py` writes the ones for `img/sixel.png`, `img/syncterm.png`
and `img/codepage.png` (see its notes); the others are here as they are.

What `img/viewdata.png`, `img/igs.png` and `img/skypix.png` are drawn
from: screens written for the site in each system's own codes (not a BBS's
art). Not published. From the repository's root:

```sh
BADTERM_VIEWDATA=site/samples/viewdata.bin BADTERM_SNAPSHOT_DIR=/tmp/shots cargo test -p badterm viewdata_snapshot -- --ignored
BADTERM_IGS=site/samples/igs.ig BADTERM_SNAPSHOT_DIR=/tmp/shots cargo test -p badterm igs_snapshot -- --ignored
BADTERM_SKYPIX=site/samples/skypix.ans BADTERM_SNAPSHOT_DIR=/tmp/shots cargo test -p badterm skypix_snapshot -- --ignored
```

Then save each as a palette PNG (64 colors) in `img/`. `igs.ig` must keep
its CRLF line ends: IGS takes the two bytes after a chained command as the
next command and its chain mark.
