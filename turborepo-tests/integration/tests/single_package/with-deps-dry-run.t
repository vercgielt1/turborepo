Setup
  $ . ${TESTDIR}/../_helpers/setup_monorepo.sh single_package_deps

Check
  $ ${TURBO} run test --dry
  
  Global Hash Inputs
    Global Files                          = 2
    External Dependencies Hash            = 
    Global Cache Key                      = HEY STELLLLLLLAAAAAAAAAAAAA
    Global .env Files Considered          = 0
    Global Env Vars                       = 
    Global Env Vars Values                = 
    Inferred Global Env Vars Values       = 
    Global Passed Through Env Vars        = 
    Global Passed Through Env Vars Values = 
  
  Tasks to Run
  build
    Task                           = build                                                                                                                                           
    Hash                           = cb6df6cef2cfd596                                                                                                                                
    Cached (Local)                 = false                                                                                                                                           
    Cached (Remote)                = false                                                                                                                                           
    Command                        = echo 'building' > foo                                                                                                                           
    Outputs                        = foo                                                                                                                                             
    Log File                       = .turbo/turbo-build.log                                                                                                                          
    Dependencies                   =                                                                                                                                                 
    Dependendents                  = test                                                                                                                                            
    Inputs Files Considered        = 4                                                                                                                                               
    .env Files Considered          = 0                                                                                                                                               
    Env Vars                       =                                                                                                                                                 
    Env Vars Values                =                                                                                                                                                 
    Inferred Env Vars Values       =                                                                                                                                                 
    Passed Through Env Vars        =                                                                                                                                                 
    Passed Through Env Vars Values =                                                                                                                                                 
    ResolvedTaskDefinition         = {"outputs":["foo"],"cache":true,"dependsOn":[],"inputs":[],"outputMode":"full","persistent":false,"env":[],"passThroughEnv":null,"dotEnv":null} 
    Framework                      = <NO FRAMEWORK DETECTED>                                                                                                                         
  test
    Task                           = test                                                                                                                                              
    Hash                           = af35177522814c73                                                                                                                                  
    Cached (Local)                 = false                                                                                                                                             
    Cached (Remote)                = false                                                                                                                                             
    Command                        = [[ ( -f foo ) && $(cat foo) == 'building' ]]                                                                                                      
    Outputs                        =                                                                                                                                                   
    Log File                       = .turbo/turbo-test.log                                                                                                                             
    Dependencies                   = build                                                                                                                                             
    Dependendents                  =                                                                                                                                                   
    Inputs Files Considered        = 4                                                                                                                                                 
    .env Files Considered          = 0                                                                                                                                                 
    Env Vars                       =                                                                                                                                                   
    Env Vars Values                =                                                                                                                                                   
    Inferred Env Vars Values       =                                                                                                                                                   
    Passed Through Env Vars        =                                                                                                                                                   
    Passed Through Env Vars Values =                                                                                                                                                   
    ResolvedTaskDefinition         = {"outputs":[],"cache":true,"dependsOn":["build"],"inputs":[],"outputMode":"full","persistent":false,"env":[],"passThroughEnv":null,"dotEnv":null} 
    Framework                      = <NO FRAMEWORK DETECTED>                                                                                                                           
