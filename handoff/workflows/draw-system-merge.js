export const meta = {
  name: 'draw-system-merge',
  description: 'Merge the finished draw-system cloud branch into main in a worktree: resolve 7 conflicts, raise the storage schema for kinds 9-11, Windows release build and full suite, review, verify, fix',
  phases: [
    { title: 'Merge', detail: 'worktree draw-merge from main, merge origin/draw-system 0bf82db, schema 12, build -j4, full suite green' },
    { title: 'Review', detail: 'three lenses: kinds and storage, merge intent, surfaces and docs' },
    { title: 'Verify', detail: "adversarial check of each lens's findings" },
    { title: 'Fix', detail: 'fix what survived, rebuild, full suite' },
  ],
}

const WT = 'C:/GitHubProjects/Katana-wt/draw-merge'
const CHECK_DIR = 'C:/GitHubProjects/Katana-wt/draw-check'
const RULES = `
RULES that bite here (CLAUDE.md has them in full):
- Work ONLY in ${WT} (and scratch folders under C:/GitHubProjects/Katana-wt/). Never touch C:/GitHubProjects/Katana's working tree, its build directory or its main branch: the owner is using them (their katana.exe is running from that build). Never push.
- A GDAL workflow is building and testing in other worktrees under C:/GitHubProjects/Katana-wt/ at the same time: build with --parallel 4 and ctest with --parallel 6. Never two builds in one build dir; a build moved to the background keeps running - wait for its notification. Show the build: no grep/tail/head over cmake/ninja/ctest output (tee to a file under ${WT}/build if you need a copy, then search the file); judge by EXIT CODE (with tee, bash's PIPESTATUS). After editing CMake files run cmake --preset release on its own first. "premature end of file; recovering" -> ninja -C build/release -t recompact, then build once. If configure tries to download GoogleTest/benchmark, copy C:/GitHubProjects/Katana/third_party/_cache into ${WT}/third_party/ first.
- Because other worktrees run their suites at the same time, a test that fails once and passes when rerun alone may be colliding on a fixed temp path: that is a real defect (tests must use unique temp paths) - fix it if small, otherwise report it; never dismiss it as flaky.
- Section 4: never change an expected value to match output; justify any moved expectation from a source outside the program; never disable a test; prove a regression test catches its bug. -Werror -Wshadow; GCC 16 here, while the branch was only built with GCC 15.3 on Linux. Qt tests: QT_QPA_PLATFORM=offscreen; fonts differ from Linux, so assert ink against a baseline, never an absolute pixel count.
- Section 11: the entity Geometry variant is append-only, its index is the on-disk kind byte, and a new kind goes at the end WITH a storage schema bump.
- Docs in the same commit, the docs test green. Commits "<area>: <what>" + why + Evidence + Outstanding, ending with the line: Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>. git diff --cached before each commit; never git add -A blindly. Naming rule (section 9).
`
const CONTEXT = `
CONTEXT: branch origin/draw-system (head 0bf82db) is a finished cloud session's work: the professional draw system and vertex control - grips, Draw > Vertices (18 tools) and the Vertices panel, polyline arcs, 3D polyline, spline, ellipse, construction lines, double line, sketch, revision cloud, object snap tracking and new snap modes, and the verbs VERTEX LIST/INSERT/DELETE/MOVE/SET, WEED, DENSIFY, STRAIGHTEN, CLOSE, OPEN, STARTVERTEX, VERTEXZ, PLINE ARC/LINE, PLINE3D, SPLINE, ELLIPSE, XLINE, RAY, DLINE, ORTHO, POLAR, TRACKING, ANGLES, LOCK, SNAP, DRAFTING (src/katana_cad/drawing/, docs/drawing.md). It appended geometry kinds CurvePolyline2 = 9, Ellipse2 = 10, Spline2 = 11 after Label 7 and Leader 8 (see its commit 7d117ca for the full fan-out) but left kCurrentSchemaVersion at 11 (include/katana/storage/project_store.hpp). Its declared behaviour changes: a plain PLINE replies with the new polyline's id instead of "polyline created"; OPEN opens polylines only with #ids or SELECTION (OPEN directory still opens a project); DXF LWPOLYLINE/POLYLINE with bulges now import as curve polylines instead of chords, ELLIPSE as an ellipse, SPLINE as a spline. It passed 4244/4244 on linux-debug with GCC 15.3; it has never been built on Windows.
It branched from main at 88b046b and last merged main at 16:43 UTC on 2026-09-25. main (b749688, or later) has since gained 84 commits, notably: PR #7 smart leaders (61ecd23 changed LeaderGeometry and the geometry blob; leader tips put ON an entity; field templates; the Leaders manager), the UI-gaps programme (MainWindow::runVerbLine / CommandRunner, the ONE executor that every dialog's verb line goes through; many new dialogs and menu items), utility-scope (the shared scope grammar include/katana/cad/scope_verbs.hpp, VIEW/AREA scopes, ScopeFilterWidget, UTILITY REGRADE/SCHEDULE), and PR #8 IFC 4.3 (katana_ifc; IMPORT/EXPORT/INFO of .ifc; File > Import/Export IFC). CLAUDE.md sections 1 and 1.1 are stricter than when the branch was written.
A trial merge (git merge-tree main origin/draw-system) conflicts in: include/katana/cad/interactive_tool.hpp, src/katana_cad/interactive_tool.cpp, include/katana/entity/geometry_blob.hpp, src/katana_entity/geometry_blob.cpp, src/katana_cad/command_interpreter.cpp, src/katana_entity/CMakeLists.txt, tests/cad/CMakeLists.txt.
`
const FINDING = { type: 'object', properties: { id: { type: 'string' }, severity: { type: 'string', enum: ['high', 'medium', 'low'] }, file: { type: 'string' }, line: { type: 'integer' }, claim: { type: 'string' }, evidence: { type: 'string' }, suggestedFix: { type: 'string' } }, required: ['id', 'severity', 'file', 'claim', 'evidence', 'suggestedFix'] }
const FINDINGS = { type: 'object', properties: { findings: { type: 'array', items: FINDING } }, required: ['findings'] }
const MERGE = {
  type: 'object',
  properties: {
    branch: { type: 'string' }, worktree: { type: 'string' }, headCommit: { type: 'string' }, mainAtMerge: { type: 'string' },
    conflictResolutions: { type: 'array', items: { type: 'object', properties: { file: { type: 'string' }, how: { type: 'string' } }, required: ['file', 'how'] } },
    schemaChange: { type: 'string' },
    testsPassed: { type: 'integer' }, testsFailed: { type: 'integer' }, failures: { type: 'array', items: { type: 'string' } },
    fixesMade: { type: 'array', items: { type: 'string' } },
    outstanding: { type: 'array', items: { type: 'string' } },
    summary: { type: 'string' },
  },
  required: ['branch', 'worktree', 'headCommit', 'mainAtMerge', 'conflictResolutions', 'schemaChange', 'testsPassed', 'testsFailed', 'failures', 'fixesMade', 'outstanding', 'summary'],
}
const VERDICTS = {
  type: 'object',
  properties: { verdicts: { type: 'array', items: { type: 'object', properties: { id: { type: 'string' }, verdict: { type: 'string', enum: ['confirmed', 'plausible', 'refuted'] }, reason: { type: 'string' }, evidence: { type: 'string' } }, required: ['id', 'verdict', 'reason', 'evidence'] } } },
  required: ['verdicts'],
}
const FIX = {
  type: 'object',
  properties: { headCommit: { type: 'string' }, fixed: { type: 'array', items: { type: 'object', properties: { id: { type: 'string' }, what: { type: 'string' } }, required: ['id', 'what'] } }, refuted: { type: 'array', items: { type: 'object', properties: { id: { type: 'string' }, why: { type: 'string' } }, required: ['id', 'why'] } }, deferred: { type: 'array', items: { type: 'object', properties: { id: { type: 'string' }, why: { type: 'string' } }, required: ['id', 'why'] } }, testsPassed: { type: 'integer' }, testsFailed: { type: 'integer' }, failures: { type: 'array', items: { type: 'string' } }, summary: { type: 'string' } },
  required: ['headCommit', 'fixed', 'refuted', 'deferred', 'testsPassed', 'testsFailed', 'failures', 'summary'],
}

phase('Merge')
const merged = await agent(`You merge the finished draw-system branch into Katana's main, in a worktree.
${RULES}
${CONTEXT}
1. git -C C:/GitHubProjects/Katana fetch origin draw-system ; confirm origin/draw-system is 0bf82db (if it moved, say so and merge the new head). git -C C:/GitHubProjects/Katana worktree add -b draw-merge ${WT} main (if the branch or worktree exists, inspect it and continue from its state). Record main's hash.
2. In ${WT}: git merge --no-ff origin/draw-system. Resolve each conflict keeping BOTH sides' intent - read both sides' commits for the file (git log 88b046b..main -- <file> and 88b046b..origin/draw-system -- <file>). In geometry_blob keep smart leaders' LeaderGeometry encoding AND the new kinds' encodings, each pinned by its byte-level tests. In command_interpreter keep every verb of both sides, the HELP text, and check the OPEN verb (project open vs polyline open) and PLINE's reply against any caller main added since (dialogs building PLINE lines through runVerbLine, tests, docs, MCP). In interactive_tool keep every tool of both sides. Then look beyond textual conflicts: grep main's code added since 88b046b for dispatch over geometry kinds (std::visit, get_if chains, switch on index) - smart leaders, IFC export/import, utility tools, Global Modify, the UI-gaps dialogs - and make each handle CurvePolyline2/Ellipse2/Spline2 sensibly, or refuse them explicitly, never silently skip them (curve pieces: entity/curve_pieces.hpp from the branch). Note DXF bulge polylines now import as CurvePolyline2, so tools that accepted those as PolylineGeometry before must keep working on them.
3. Storage schema: the three kinds need a bump. Follow docs/model.md and docs/storage.md, and how the annotation system took the schema to 11 (find that commit): raise kCurrentSchemaVersion to 12 with whatever migration and version checks the store has, and tests - a schema-11 project still opens; a project holding kinds 9-11 is written as 12; an older reader's refusal path is honest. Update the docs' schema history.
4. Commit the merge (message: what conflicted and how each was resolved, the schema change, Evidence, Outstanding).
5. cmake --preset release ; cmake --build build/release --parallel 4 ; the FULL suite (ctest --parallel 6). Fix every failure properly (section 4), commit each fix with its evidence, rerun until green. Confirm build/release/bin/katana.exe is newer than the sources.
6. Record, under "Not done" in docs/drawing.md, what the draw verbs and dialogs still lack against CLAUDE.md section 1.1 (the shared scope and filter: their targets are #ids or SELECTION only) and section 1 (dialogs that do not yet go through runVerbLine), and add those tools to C:/GitHubProjects/Katana-wt/scope-retrofit-audit.json (read its shape first; append entries in the same form, do not rewrite the others). Do NOT do the scope retrofit itself.
Return the structured result.`, { label: 'merge draw-system', phase: 'Merge', schema: MERGE })
log(`merge: ${merged ? `${merged.testsPassed} passed / ${merged.testsFailed} failed at ${merged.headCommit}; schema: ${merged.schemaChange}` : 'FAILED'}`)
if (!merged) {
  return { stopped: 'merge failed' }
}

const SHARED = `Do NOT edit source, build or commit. Nothing builds in ${WT} while you work, but two other reviewers use the same binaries: never run the whole suite (a narrow ctest -R, or a binary run under timeout with QT_QPA_PLATFORM=offscreen, is fine), and work only in your own folder under ${CHECK_DIR} on COPIES of data (samples/site_plan, or DXF/projects you write).`
const base = `Review the merge of the draw-system branch into Katana's main, in ${WT} (branch draw-merge, built at ${merged.headCommit}, suite ${merged.testsPassed} passed / ${merged.testsFailed} failed). The merge agent reported: ${JSON.stringify(merged).slice(0, 12000)}
${CONTEXT}
Read the merge commit (git -C ${WT} show --cc <merge>), the conflict resolutions, and the fix commits after it; read touched files whole where needed. ${SHARED} Your folder: ${CHECK_DIR}/<your lens>. Report only real defects with concrete evidence (file:line, code, the input that breaks it); prefix ids with your lens. Things already recorded as "Not done" in docs/drawing.md are not findings.`
const LENSES = [
  { key: 'kinds', lens: `LENS "kinds": the three new geometry kinds end to end on the merged tree. The kind bytes (9, 10, 11 after 7 and 8), the geometry blob for every kind including smart leaders' new LeaderGeometry layout, the schema bump to 12 and opening a schema-11 project, JSON, DXF round trips, 12da archive export, IFC export (PR #8) and every other place main's new code dispatches over geometry kinds - smart leaders reading the entity a tip is ON, utility tools, Global Modify, snaps, selection, the 3D scene, plotting. A curve polyline, ellipse or spline silently dropped, drawn as chords where the geometry says arcs, or written wrongly is a finding; prove it with a file or a katana_cli line.` },
  { key: 'merge', lens: `LENS "merge": did each conflict resolution keep BOTH sides' intent? interactive_tool (every tool of both sides still registered and reachable), command_interpreter (every verb of both sides, HELP complete, OPEN for projects vs polylines, PLINE's new reply vs any caller or test main added, the UI-gaps runVerbLine path, katana_mcp), the CMake lists (tests/qt_widgets and benchmarks/qt explicit source lists), main_window and the menus (every item reachable, menu letters unique - qt_every_shortcut_and_menu_letter_reaches_one_thing_headless), docs/index.md. Also a test that would pass if the merged code were wrong, and a silent failure introduced by the merge.` },
  { key: 'surfaces', lens: `LENS "surfaces": the draw system against CLAUDE.md section 1 as main now enforces it - each feature reachable in the window (menu item, objectNames on every widget, dialogs), katana_cli and katana_mcp; key=value replies; one undo step per edit; no hidden modality (a headless run never blocks); HELP, the Command Reference and docs/drawing.md true of the merged code; the naming rule; the docs test. Check that the section 1 / 1.1 gaps are recorded honestly in docs/drawing.md "Not done" and in C:/GitHubProjects/Katana-wt/scope-retrofit-audit.json, and that nothing else is missing from that record. Drive the window headlessly (build/release/bin/katana.exe --action/--dialog/--run-line/--screenshot, docs/headless.md) for the Draw menu and the Vertices panel, and read the PNG.` },
]
const verifyPrompt = (key, fs) => `You adversarially check the findings of the "${key}" review of the draw-system merge into Katana, in ${WT} (branch draw-merge, built at ${merged.headCommit}).
${SHARED} Your folder: ${CHECK_DIR}/verify-${key}.
For EACH finding try to REFUTE it: read the code at and around the cited place, and reproduce the claimed failure with the input given (a katana_cli line, a headless katana.exe run, a narrow ctest -R). Verdict: confirmed (reproduced, or the code plainly does it), plausible (the code reads that way but you could not reproduce it), refuted (the evidence does not hold - say why). When torn between plausible and refuted, choose refuted and say why. One verdict per finding id.
Findings: ${JSON.stringify(fs, null, 1).slice(0, 60000)}`

const checked = await pipeline(LENSES,
  l => agent(`${base}\n${l.lens}`, { label: `review:${l.key}`, phase: 'Review', schema: FINDINGS }),
  async (r, l) => {
    if (!r) {
      log(`review:${l.key}: FAILED (no result)`)
      return { l, ok: false, kept: [], refuted: [] }
    }
    if (r.findings.length === 0) {
      log(`review:${l.key}: no findings`)
      return { l, ok: true, kept: [], refuted: [] }
    }
    const v = await agent(verifyPrompt(l.key, r.findings), { label: `verify:${l.key}`, phase: 'Verify', schema: VERDICTS })
    const byId = new Map((v ? v.verdicts : []).map(x => [x.id, x]))
    const kept = [], refuted = []
    for (const f of r.findings) {
      const x = byId.get(f.id)
      if (x && x.verdict === 'refuted') refuted.push({ ...f, refutation: x.reason, refutationEvidence: x.evidence })
      else kept.push({ ...f, verdict: x ? `${x.verdict}: ${x.reason} | ${x.evidence}` : 'unverified (no verdict returned)' })
    }
    log(`review:${l.key}: ${r.findings.length} findings, ${kept.length} survived, ${refuted.length} refuted`)
    return { l, ok: true, kept, refuted }
  })
const done = checked.filter(Boolean)
const kept = done.flatMap(x => x.kept)
const refuted = done.flatMap(x => x.refuted)
const failedReviews = LENSES.filter((l, i) => !checked[i] || !checked[i].ok).map(l => l.key)
if (failedReviews.length) log(`reviews with no result: ${failedReviews.join(', ')}`)

phase('Fix')
const fixed = kept.length === 0 ? { headCommit: merged.headCommit, fixed: [], refuted: [], deferred: [], testsPassed: merged.testsPassed, testsFailed: merged.testsFailed, failures: merged.failures, summary: 'no findings survived' } :
  await agent(`You finish the merge of the draw-system branch into Katana, in ${WT} (branch draw-merge, built at ${merged.headCommit}).
${RULES}
${CONTEXT}
Each finding below survived an adversarial check (its verdict says how). Still read before changing anything; if one proves wrong, list it as refuted with the evidence. The section 1.1 scope retrofit of the draw verbs is NOT in scope - only make sure it is recorded.
Findings: ${JSON.stringify(kept, null, 1).slice(0, 90000)}
Fix every real one properly, with a test that fails before and passes after where the defect is behavioural; commit in small groups, docs in the same commit. Defer only what is genuinely out of scope, saying why, and record it under "Not done" in the area's doc. Then cmake --preset release (if CMake files changed), build --parallel 4, the FULL suite (--parallel 6) green, and build/release/bin/katana.exe newer than every source you changed.
Return the structured result.`, { label: 'fix', phase: 'Fix', schema: FIX })
log(`fix: ${fixed ? `${fixed.fixed.length} fixed, ${fixed.refuted.length} refuted, ${fixed.deferred.length} deferred; ${fixed.testsPassed} passed / ${fixed.testsFailed} failed at ${fixed.headCommit}` : 'FAILED'}`)

return { merged, kept, refuted, failedReviews, fixed }
