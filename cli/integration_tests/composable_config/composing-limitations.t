Setup
  $ . ${TESTDIR}/../setup.sh
  $ . ${TESTDIR}/setup.sh $(pwd) ./monorepo

  $ ${TURBO} run build --filter=package-task
   ERROR  run failed: error preparing engine: turbo.json failed validation
  Turbo error: error preparing engine: turbo.json failed validation
  [1]
