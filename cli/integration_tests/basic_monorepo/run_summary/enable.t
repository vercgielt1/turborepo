Setup
  $ . ${TESTDIR}/../../setup.sh
  $ . ${TESTDIR}/../setup.sh $(pwd)

# Delete all run summaries to start
  $ rm -rf .turbo/runs

# Tests
| env var | flag    | summary? |
| ------- | ------- | -------- |
| true    | missing | yes      |
| true    | true    | yes      |
| true    | false   | no       |

| false   | missing | no       |
| false   | true    | yes      |
| false   | false   | no       |

| missing | missing | no       |
| missing | true    | yes      |
| missing | false   | no       |


# env var=true, no flag: yes
  $ rm -rf .turbo/runs
  $ TURBO_RUN_SUMMARY=true ${TURBO} run build > /dev/null
  $ /bin/ls .turbo/runs/*.json | wc -l
  \s*1 (re)
# env var=true, --flag=true: yes
  $ rm -rf .turbo/runs
  $ TURBO_RUN_SUMMARY=true ${TURBO} run build > /dev/null
  $ /bin/ls .turbo/runs/*.json | wc -l
  \s*1 (re)
# env var=true, --flag=false: no
  $ rm -rf .turbo/runs
  $ TURBO_RUN_SUMMARY=true ${TURBO} run build > /dev/null
  $ test -d .turbo/runs
  [1]

# env var=false, no flag, no
  $ rm -rf .turbo/runs
  $ TURBO_RUN_SUMMARY=false ${TURBO} run build > /dev/null
  $ test -d .turbo/runs
  [1]
# env var=false, --flag=true: yes
  $ rm -rf .turbo/runs
  $ TURBO_RUN_SUMMARY=false ${TURBO} run build > /dev/null
  $ /bin/ls .turbo/runs/*.json | wc -l
  \s*1 (re)
# env var=false, --flag=false: no
  $ rm -rf .turbo/runs
  $ TURBO_RUN_SUMMARY=false ${TURBO} run build > /dev/null
  $ test -d .turbo/runs
  [1]

# no env var, no flag: no
  $ rm -rf .turbo/runs
  $ ${TURBO} run build > /dev/null
  $ test -d .turbo/runs
  [1]
# no env var, --flag=true: yes
  $ rm -rf .turbo/runs
  $ ${TURBO} run build > /dev/null
  $ /bin/ls .turbo/runs/*.json | wc -l
  \s*1 (re)
# no env var, --flag=false: no
  $ rm -rf .turbo/runs
  $ ${TURBO} run build > /dev/null
  $ test -d .turbo/runs
  [1]
