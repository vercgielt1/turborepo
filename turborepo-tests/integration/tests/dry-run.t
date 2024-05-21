Setup
  $ . ${TESTDIR}/../../helpers/setup_integration_test.sh

# Run the build task with --dry flag and cut up the logs into separate files by empty lines
# https://stackoverflow.com/a/33297878/986415
  $ ${TURBO} run build --dry |  awk -v RS= '{print > ("tmp-" NR ".txt")}'

# The first part of the file is Packages in Scope
  $ cat tmp-1.txt
  Packages in Scope
  Name    Path\s* (re)
  another packages(\/|\\)another\s* (re)
  my-app  apps(\/|\\)my-app\s* (re)
  util    packages(\/|\\)util\s* (re)

# Part 2 of the logs are Global Hash INputs
  $ cat tmp-2.txt
  Global Hash Inputs
    Global Files                          = 1
    External Dependencies Hash            = 459c029558afe716
    Global Cache Key                      = I can\xe2\x80\x99t see ya, but I know you\xe2\x80\x99re here (esc)
    Global Env Vars                       = SOME_ENV_VAR
    Global Env Vars Values                = 
    Inferred Global Env Vars Values       = 
    Global Passed Through Env Vars        = 
    Global Passed Through Env Vars Values = 

# Part 3 are Tasks to Run, and we have to validate each task separately
  $ cat tmp-3.txt | grep "my-app#build" -A 17
  my-app#build
    Task                           = build\s* (re)
    Package                        = my-app\s* (re)
    Hash                           = ed450f573b231cb7
    Cached (Local)                 = false
    Cached (Remote)                = false
    Directory                      = apps/my-app
    Command                        = echo building
    Outputs                        = apple.json, banana.txt
    Log File                       = apps/my-app/.turbo/turbo-build.log
    Dependencies                   = 
    Dependents                     = 
    Inputs Files Considered        = 2
    Env Vars                       = 
    Env Vars Values                = 
    Inferred Env Vars Values       = 
    Passed Through Env Vars        = 
    Passed Through Env Vars Values = 

  $ cat tmp-3.txt | grep "util#build" -A 17
  util#build
    Task                           = build\s* (re)
    Package                        = util\s* (re)
    Hash                           = 41b033e352a43533
    Cached (Local)                 = false
    Cached (Remote)                = false
    Directory                      = packages/util
    Command                        = echo building
    Outputs                        = 
    Log File                       = packages/util/.turbo/turbo-build.log
    Dependencies                   = 
    Dependents                     = 
    Inputs Files Considered        = 1
    Env Vars                       = NODE_ENV
    Env Vars Values                = 
    Inferred Env Vars Values       = 
    Passed Through Env Vars        = 
    Passed Through Env Vars Values = 

# Run the task with NODE_ENV set and see it in summary. Use util package so it's just one package
  $ NODE_ENV=banana ${TURBO} run build --dry --filter=util | grep "Environment Variables"
  [1]
