Setup
  $ . ${TESTDIR}/../_helpers/setup.sh
  $ . ${TESTDIR}/../_helpers/setup_monorepo.sh $(pwd)

Baseline global hash
  $ cp "$TESTDIR/fixture-configs/1-baseline.json" "$(pwd)/turbo.json" && git commit -am "no comment" --quiet
  $ BASELINE=$(${TURBO} build -vv 2>&1 | "$TESTDIR/../_helpers/get-global-hash.sh")

Update pipeline: global hash remains stable.
  $ cp "$TESTDIR/fixture-configs/2-update-pipeline.json" "$(pwd)/turbo.json" && git commit -am "no comment" --quiet
  $ UPDATE_PIPELINE=$(${TURBO} build -vv --experimental-env-mode=infer 2>&1 | "$TESTDIR/../_helpers/get-global-hash.sh")
  $ test $BASELINE = $UPDATE_PIPELINE

Update globalEnv: global hash changes.
  $ cp "$TESTDIR/fixture-configs/3-update-global-env.json" "$(pwd)/turbo.json" && git commit -am "no comment" --quiet
  $ NEW_GLOBAL_ENV=$(${TURBO} build -vv --experimental-env-mode=infer 2>&1 | "$TESTDIR/../_helpers/get-global-hash.sh")
  $ test $BASELINE != $NEW_GLOBAL_ENV

Update globalDeps in a non-material way: global hash remains stable.
  $ cp "$TESTDIR/fixture-configs/4-update-global-deps.json" "$(pwd)/turbo.json" && git commit -am "no comment" --quiet
  $ NEW_GLOBAL_DEPS=$(${TURBO} build -vv --experimental-env-mode=infer 2>&1 | "$TESTDIR/../_helpers/get-global-hash.sh")
  $ test $BASELINE = $NEW_GLOBAL_DEPS

Update globalDeps in a material way: global hash changes.
  $ cp "$TESTDIR/fixture-configs/5-update-global-deps-materially.json" "$(pwd)/turbo.json" && git commit -am "no comment" --quiet
  $ NEW_GLOBAL_DEPS=$(${TURBO} build -vv --experimental-env-mode=infer 2>&1 | "$TESTDIR/../_helpers/get-global-hash.sh")
  $ test $BASELINE != $NEW_GLOBAL_DEPS

Update passthroughEnv: global hash changes.
  $ cp "$TESTDIR/fixture-configs/6-update-passthrough-env.json" "$(pwd)/turbo.json" && git commit -am "no comment" --quiet
  $ NEW_GLOBAL_PASSTHROUGH=$(${TURBO} build -vv --experimental-env-mode=infer 2>&1 | "$TESTDIR/../_helpers/get-global-hash.sh")
  $ test $BASELINE != $NEW_GLOBAL_PASSTHROUGH
