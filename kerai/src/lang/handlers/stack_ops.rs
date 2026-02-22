use crate::lang::machine::Machine;
use crate::lang::ptr::Ptr;

/// Duplicate the top stack item.
pub fn dup(m: &mut Machine) -> Result<(), String> {
    let top = m.peek().ok_or("dup: stack empty")?.clone();
    m.push(top);
    Ok(())
}

/// Remove the top stack item.
pub fn drop(m: &mut Machine) -> Result<(), String> {
    m.pop().ok_or("drop: stack empty")?;
    Ok(())
}

/// Swap the top two stack items.
pub fn swap(m: &mut Machine) -> Result<(), String> {
    if m.depth() < 2 {
        return Err("swap: need at least 2 items".into());
    }
    let len = m.stack.len();
    m.stack.swap(len - 1, len - 2);
    Ok(())
}

/// Copy the second item to the top: a b → a b a
pub fn over(m: &mut Machine) -> Result<(), String> {
    if m.depth() < 2 {
        return Err("over: need at least 2 items".into());
    }
    let second = m.stack[m.stack.len() - 2].clone();
    m.push(second);
    Ok(())
}

/// Rotate the top three items: a b c → b c a
pub fn rot(m: &mut Machine) -> Result<(), String> {
    if m.depth() < 3 {
        return Err("rot: need at least 3 items".into());
    }
    let len = m.stack.len();
    let a = m.stack.remove(len - 3);
    m.push(a);
    Ok(())
}

/// Clear the entire stack.
pub fn clear(m: &mut Machine) -> Result<(), String> {
    m.stack.clear();
    Ok(())
}

/// Mark the top item for expanded view (sets meta.view = true).
pub fn view(m: &mut Machine) -> Result<(), String> {
    let top = m.stack.last_mut().ok_or("view: stack empty")?;
    if let Some(obj) = top.meta.as_object_mut() {
        obj.insert("view".into(), serde_json::Value::Bool(true));
    } else {
        top.meta = serde_json::json!({"view": true});
    }
    Ok(())
}

/// Push the stack depth as an integer.
pub fn depth(m: &mut Machine) -> Result<(), String> {
    let d = m.depth() as i64;
    m.push(Ptr::int(d));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_stack(items: Vec<Ptr>) -> Machine {
        let (handlers, type_methods, help) = crate::lang::handlers::register_all();
        let mut m = Machine::new(uuid::Uuid::nil(), uuid::Uuid::nil(), handlers, type_methods, help);
        m.stack = items;
        m
    }

    #[test]
    fn test_over() {
        let mut m = make_stack(vec![Ptr::int(1), Ptr::int(2)]);
        over(&mut m).unwrap();
        assert_eq!(m.stack.len(), 3);
        assert_eq!(m.stack[2], Ptr::int(1));
    }

    #[test]
    fn test_rot() {
        let mut m = make_stack(vec![Ptr::int(1), Ptr::int(2), Ptr::int(3)]);
        rot(&mut m).unwrap();
        // 1 2 3 → 2 3 1
        assert_eq!(m.stack[0], Ptr::int(2));
        assert_eq!(m.stack[1], Ptr::int(3));
        assert_eq!(m.stack[2], Ptr::int(1));
    }

    #[test]
    fn test_depth() {
        let mut m = make_stack(vec![Ptr::int(1), Ptr::int(2)]);
        depth(&mut m).unwrap();
        assert_eq!(m.stack.len(), 3);
        assert_eq!(m.stack[2], Ptr::int(2));
    }
}
