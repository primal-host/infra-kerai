use crate::lang::machine::Machine;
use crate::lang::ptr::Ptr;

/// Pop two numeric items, apply op, push result.
fn binary_op(m: &mut Machine, op: &str) -> Result<(), String> {
    if m.depth() < 2 {
        return Err(format!("{op}: need at least 2 items"));
    }

    let b = m.pop().unwrap();
    let a = m.pop().unwrap();

    if !a.is_numeric() || !b.is_numeric() {
        // Push both back and error
        m.push(a);
        m.push(b);
        return Err(format!("{op}: both operands must be numeric"));
    }

    // Both int → int result
    if a.kind == "int" && b.kind == "int" {
        let av = a.as_int().unwrap();
        let bv = b.as_int().unwrap();
        m.push(int_op(op, av, bv));
    } else {
        // Promote to float
        let av = a.as_float().unwrap();
        let bv = b.as_float().unwrap();
        m.push(float_op(op, av, bv));
    }

    Ok(())
}

fn int_op(op: &str, a: i64, b: i64) -> Ptr {
    match op {
        "+" => Ptr::int(a.wrapping_add(b)),
        "-" => Ptr::int(a.wrapping_sub(b)),
        "*" => Ptr::int(a.wrapping_mul(b)),
        "/" => {
            if b == 0 {
                Ptr::error("division by zero")
            } else {
                Ptr::int(a / b)
            }
        }
        "%" => {
            if b == 0 {
                Ptr::error("division by zero")
            } else {
                Ptr::int(a % b)
            }
        }
        _ => Ptr::error(&format!("unknown op: {op}")),
    }
}

fn float_op(op: &str, a: f64, b: f64) -> Ptr {
    match op {
        "+" => Ptr::float(a + b),
        "-" => Ptr::float(a - b),
        "*" => Ptr::float(a * b),
        "/" => {
            if b == 0.0 {
                Ptr::error("division by zero")
            } else {
                Ptr::float(a / b)
            }
        }
        "%" => {
            if b == 0.0 {
                Ptr::error("division by zero")
            } else {
                Ptr::float(a % b)
            }
        }
        _ => Ptr::error(&format!("unknown op: {op}")),
    }
}

pub fn add(m: &mut Machine) -> Result<(), String> {
    binary_op(m, "+")
}

pub fn sub(m: &mut Machine) -> Result<(), String> {
    binary_op(m, "-")
}

pub fn mul(m: &mut Machine) -> Result<(), String> {
    binary_op(m, "*")
}

pub fn div(m: &mut Machine) -> Result<(), String> {
    binary_op(m, "/")
}

pub fn modulo(m: &mut Machine) -> Result<(), String> {
    binary_op(m, "%")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::handlers::register_all;

    fn make_machine() -> Machine {
        let (handlers, type_methods, help) = register_all();
        Machine::new(uuid::Uuid::nil(), uuid::Uuid::nil(), handlers, type_methods, help)
    }

    #[test]
    fn add_ints() {
        let mut m = make_machine();
        m.execute("10 20 +").unwrap();
        assert_eq!(m.stack[0], Ptr::int(30));
    }

    #[test]
    fn sub_ints() {
        let mut m = make_machine();
        m.execute("10 3 -").unwrap();
        assert_eq!(m.stack[0], Ptr::int(7));
    }

    #[test]
    fn mul_floats() {
        let mut m = make_machine();
        m.execute("2.5 4 *").unwrap();
        assert_eq!(m.stack[0].kind, "float");
        assert_eq!(m.stack[0].as_float(), Some(10.0));
    }

    #[test]
    fn div_by_zero_int() {
        let mut m = make_machine();
        m.execute("5 0 /").unwrap();
        assert_eq!(m.stack[0].kind, "error");
        assert_eq!(m.stack[0].ref_id, "division by zero");
    }

    #[test]
    fn modulo_works() {
        let mut m = make_machine();
        m.execute("10 3 %").unwrap();
        assert_eq!(m.stack[0], Ptr::int(1));
    }

    #[test]
    fn non_numeric_error() {
        let mut m = make_machine();
        m.push(Ptr::text("hello"));
        m.push(Ptr::int(1));
        let result = add(&mut m);
        assert!(result.is_err());
        // Both operands should be pushed back
        assert_eq!(m.stack.len(), 2);
    }
}
