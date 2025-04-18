// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.

use std::any::Any;
use std::sync::Arc;

use arrow::array::{ArrayRef, GenericStringArray, OffsetSizeTrait};
use arrow::datatypes::DataType;

use crate::utils::{make_scalar_function, utf8_to_str_type};
use datafusion_common::cast::{
    as_generic_string_array, as_int64_array, as_string_view_array,
};
use datafusion_common::{exec_err, Result};
use datafusion_expr::{ColumnarValue, Documentation, TypeSignature, Volatility};
use datafusion_expr::{ScalarFunctionArgs, ScalarUDFImpl, Signature};
use datafusion_macros::user_doc;

#[user_doc(
    doc_section(label = "String Functions"),
    description = "Returns the string which is replaced by another string from the specified position and specified count length.",
    syntax_example = "overlay(str PLACING substr FROM pos [FOR count])",
    sql_example = r#"```sql
> select overlay('Txxxxas' placing 'hom' from 2 for 4);
+--------------------------------------------------------+
| overlay(Utf8("Txxxxas"),Utf8("hom"),Int64(2),Int64(4)) |
+--------------------------------------------------------+
| Thomas                                                 |
+--------------------------------------------------------+
```"#,
    standard_argument(name = "str", prefix = "String"),
    argument(name = "substr", description = "Substring to replace in str."),
    argument(
        name = "pos",
        description = "The start position to start the replace in str."
    ),
    argument(
        name = "count",
        description = "The count of characters to be replaced from start position of str. If not specified, will use substr length instead."
    )
)]
#[derive(Debug)]
pub struct OverlayFunc {
    signature: Signature,
}

impl Default for OverlayFunc {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayFunc {
    pub fn new() -> Self {
        use DataType::*;
        Self {
            signature: Signature::one_of(
                vec![
                    TypeSignature::Exact(vec![Utf8View, Utf8View, Int64, Int64]),
                    TypeSignature::Exact(vec![Utf8, Utf8, Int64, Int64]),
                    TypeSignature::Exact(vec![LargeUtf8, LargeUtf8, Int64, Int64]),
                    TypeSignature::Exact(vec![Utf8View, Utf8View, Int64]),
                    TypeSignature::Exact(vec![Utf8, Utf8, Int64]),
                    TypeSignature::Exact(vec![LargeUtf8, LargeUtf8, Int64]),
                ],
                Volatility::Immutable,
            ),
        }
    }
}

impl ScalarUDFImpl for OverlayFunc {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> &str {
        "overlay"
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, arg_types: &[DataType]) -> Result<DataType> {
        utf8_to_str_type(&arg_types[0], "overlay")
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        match args.args[0].data_type() {
            DataType::Utf8View | DataType::Utf8 => {
                make_scalar_function(overlay::<i32>, vec![])(&args.args)
            }
            DataType::LargeUtf8 => {
                make_scalar_function(overlay::<i64>, vec![])(&args.args)
            }
            other => exec_err!("Unsupported data type {other:?} for function overlay"),
        }
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

macro_rules! process_overlay {
    // For the three-argument case
    ($string_array:expr, $characters_array:expr, $pos_num:expr) => {{
        $string_array
        .iter()
        .zip($characters_array.iter())
        .zip($pos_num.iter())
        .map(|((string, characters), start_pos)| {
            match (string, characters, start_pos) {
                (Some(string), Some(characters), Some(start_pos)) => {
                    let string_len = string.chars().count();
                    let characters_len = characters.chars().count();
                    let replace_len = characters_len as i64;
                    let mut res =
                        String::with_capacity(string_len.max(characters_len));

                    //as sql replace index start from 1 while string index start from 0
                    if start_pos > 1 && start_pos - 1 < string_len as i64 {
                        let start = (start_pos - 1) as usize;
                        res.push_str(&string[..start]);
                    }
                    res.push_str(characters);
                    // if start + replace_len - 1 >= string_length, just to string end
                    if start_pos + replace_len - 1 < string_len as i64 {
                        let end = (start_pos + replace_len - 1) as usize;
                        res.push_str(&string[end..]);
                    }
                    Ok(Some(res))
                }
                _ => Ok(None),
            }
        })
        .collect::<Result<GenericStringArray<T>>>()
    }};

    // For the four-argument case
    ($string_array:expr, $characters_array:expr, $pos_num:expr, $len_num:expr) => {{
        $string_array
        .iter()
        .zip($characters_array.iter())
        .zip($pos_num.iter())
        .zip($len_num.iter())
        .map(|(((string, characters), start_pos), len)| {
            match (string, characters, start_pos, len) {
                (Some(string), Some(characters), Some(start_pos), Some(len)) => {
                    let string_len = string.chars().count();
                    let characters_len = characters.chars().count();
                    let replace_len = len.min(string_len as i64);
                    let mut res =
                        String::with_capacity(string_len.max(characters_len));

                    //as sql replace index start from 1 while string index start from 0
                    if start_pos > 1 && start_pos - 1 < string_len as i64 {
                        let start = (start_pos - 1) as usize;
                        res.push_str(&string[..start]);
                    }
                    res.push_str(characters);
                    // if start + replace_len - 1 >= string_length, just to string end
                    if start_pos + replace_len - 1 < string_len as i64 {
                        let end = (start_pos + replace_len - 1) as usize;
                        res.push_str(&string[end..]);
                    }
                    Ok(Some(res))
                }
                _ => Ok(None),
            }
        })
        .collect::<Result<GenericStringArray<T>>>()
    }};
}

/// OVERLAY(string1 PLACING string2 FROM integer FOR integer2)
/// Replaces a substring of string1 with string2 starting at the integer bit
/// pgsql overlay('Txxxxas' placing 'hom' from 2 for 4) → Thomas
/// overlay('Txxxxas' placing 'hom' from 2) -> Thomxas, without for option, str2's len is instead
fn overlay<T: OffsetSizeTrait>(args: &[ArrayRef]) -> Result<ArrayRef> {
    let use_string_view = args[0].data_type() == &DataType::Utf8View;
    if use_string_view {
        string_view_overlay::<T>(args)
    } else {
        string_overlay::<T>(args)
    }
}

pub fn string_overlay<T: OffsetSizeTrait>(args: &[ArrayRef]) -> Result<ArrayRef> {
    match args.len() {
        3 => {
            let string_array = as_generic_string_array::<T>(&args[0])?;
            let characters_array = as_generic_string_array::<T>(&args[1])?;
            let pos_num = as_int64_array(&args[2])?;

            let result = process_overlay!(string_array, characters_array, pos_num)?;
            Ok(Arc::new(result) as ArrayRef)
        }
        4 => {
            let string_array = as_generic_string_array::<T>(&args[0])?;
            let characters_array = as_generic_string_array::<T>(&args[1])?;
            let pos_num = as_int64_array(&args[2])?;
            let len_num = as_int64_array(&args[3])?;

            let result =
                process_overlay!(string_array, characters_array, pos_num, len_num)?;
            Ok(Arc::new(result) as ArrayRef)
        }
        other => {
            exec_err!("overlay was called with {other} arguments. It requires 3 or 4.")
        }
    }
}

pub fn string_view_overlay<T: OffsetSizeTrait>(args: &[ArrayRef]) -> Result<ArrayRef> {
    match args.len() {
        3 => {
            let string_array = as_string_view_array(&args[0])?;
            let characters_array = as_string_view_array(&args[1])?;
            let pos_num = as_int64_array(&args[2])?;

            let result = process_overlay!(string_array, characters_array, pos_num)?;
            Ok(Arc::new(result) as ArrayRef)
        }
        4 => {
            let string_array = as_string_view_array(&args[0])?;
            let characters_array = as_string_view_array(&args[1])?;
            let pos_num = as_int64_array(&args[2])?;
            let len_num = as_int64_array(&args[3])?;

            let result =
                process_overlay!(string_array, characters_array, pos_num, len_num)?;
            Ok(Arc::new(result) as ArrayRef)
        }
        other => {
            exec_err!("overlay was called with {other} arguments. It requires 3 or 4.")
        }
    }
}

#[cfg(test)]
mod tests {
    use arrow::array::{Int64Array, StringArray};

    use super::*;

    #[test]
    fn to_overlay() -> Result<()> {
        let string =
            Arc::new(StringArray::from(vec!["123", "abcdefg", "xyz", "Txxxxas"]));
        let replace_string =
            Arc::new(StringArray::from(vec!["abc", "qwertyasdfg", "ijk", "hom"]));
        let start = Arc::new(Int64Array::from(vec![4, 1, 1, 2])); // start
        let end = Arc::new(Int64Array::from(vec![5, 7, 2, 4])); // replace len

        let res = overlay::<i32>(&[string, replace_string, start, end]).unwrap();
        let result = as_generic_string_array::<i32>(&res).unwrap();
        let expected = StringArray::from(vec!["abc", "qwertyasdfg", "ijkz", "Thomas"]);
        assert_eq!(&expected, result);

        Ok(())
    }
}
