Setup
  $ . ${TESTDIR}/../setup.sh
  $ . ${TESTDIR}/setup.sh $(pwd)

Check
  $ ${TURBO} run test --dry --single-package
  
  Tasks to Run
  build
    Task            = build                  
    Hash            = ad0ecb9fac7f41b0       
    Cached (Local)  = false                  
    Cached (Remote) = false                  
    Command         = echo 'building' > foo  
    Outputs         = foo                    
    Log File        = .turbo/turbo-build.log 
    Dependencies    =                        
    Dependendents   = test                   
  test
    Task            = test                                         
    Hash            = ce522d29d583b888                             
    Cached (Local)  = false                                        
    Cached (Remote) = false                                        
    Command         = [[ ( -f foo ) && $(cat foo) == 'building' ]] 
    Outputs         =                                              
    Log File        = .turbo/turbo-test.log                        
    Dependencies    = build                                        
    Dependendents   =                                              

  $ ${TURBO} run test --dry=json --single-package
  {
    "tasks": [
      {
        "task": "build",
        "hash": "ad0ecb9fac7f41b0",
        "command": "echo 'building' \u003e foo",
        "outputs": [
          "foo"
        ],
        "excludedOutputs": null,
        "logFile": ".turbo/turbo-build.log",
        "dependencies": [],
        "dependents": [
          "test"
        ]
      },
      {
        "task": "test",
        "hash": "ce522d29d583b888",
        "command": "[[ ( -f foo ) \u0026\u0026 $(cat foo) == 'building' ]]",
        "outputs": null,
        "excludedOutputs": null,
        "logFile": ".turbo/turbo-test.log",
        "dependencies": [
          "build"
        ],
        "dependents": []
      }
    ]
  }
