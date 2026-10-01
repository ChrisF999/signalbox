# The Waterloo & City Working Timetable (not in this repository)

Drain can run the real London Underground Waterloo & City line timetable:
WTT No. 7, Mondays to Fridays, from 9 October 2017 (a TfL document). Only the
code that reads it is in this repository (`crates/ts2-import/src/wtt.rs`);
the PDF, its text and anything made from them are never committed (this
directory ignores everything but this file and its `.gitignore`).

To build an image with it, put your copy here as the only PDF:

    external/wtt/wtt-7-waterloo-and-city-2017-10-09.pdf

(any name ending `.pdf` in any case; two PDFs, or any other file here but
this README, `.gitignore` and a local `wtt.bbox.html`, stop the image build; sha256
`7709d5b56564dd5b0d9acd7d27cb2668fc5a88407d6dae6f389a8e6fb592475b` for the
copy this was written against). `deploy/Dockerfile` turns it into
`pdftotext -bbox` text and converts Drain with `ts2-import --wtt`, which
checks the timetable against the WTT's own figures (running times, train
workings, trains in service, service intervals) and stops the build if they
do not match. Without a PDF the image's Drain keeps its TS2 timetable.

The image then holds a timetable made from TfL's document: keep it on ra,
never push it to a public registry.

To try it outside Docker (poppler-utils installed):

    pdftotext -bbox external/wtt/*.pdf external/wtt/wtt.bbox.html
    scripts/cargo run -p ts2-import -- crates/ts2-import/tests/data/drain.json -o /w/target/drain.json \
      --areas /w/layouts/drain.areas.json --lines /w/layouts/drain.lines.json --wtt /w/external/wtt/wtt.bbox.html
    scripts/cargo test --release -p ts2-import --test wtt_day -- --ignored --nocapture

The whole-day soak (`wtt_day`) runs the day under the robot over five
seeds; it needs the robot's standing rule (polish spec P22), without which
the morning peak gridlocks.
