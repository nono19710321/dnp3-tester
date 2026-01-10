
fn main() {
    probe_database();
}

fn probe_database() {
    use dnp3::outstation::database::{Database, DatabaseHandle};
    
    let mut handle: DatabaseHandle = unsafe { std::mem::zeroed() };
    
    // PROBE 1: Relationship
    // let db: Database = handle; // Uncommenting this would show type mismatch
    
    // PROBE 2: Methods on Database Struct
    let mut db: Database = unsafe { std::mem::zeroed() };
    db.list_methods_please(); 
}
