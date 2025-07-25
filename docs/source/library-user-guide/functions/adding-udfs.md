<!---
  Licensed to the Apache Software Foundation (ASF) under one
  or more contributor license agreements.  See the NOTICE file
  distributed with this work for additional information
  regarding copyright ownership.  The ASF licenses this file
  to you under the Apache License, Version 2.0 (the
  "License"); you may not use this file except in compliance
  with the License.  You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

  Unless required by applicable law or agreed to in writing,
  software distributed under the License is distributed on an
  "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
  KIND, either express or implied.  See the License for the
  specific language governing permissions and limitations
  under the License.
-->

# Adding User Defined Functions: Scalar/Window/Aggregate/Table Functions

User Defined Functions (UDFs) are functions that can be used in the context of DataFusion execution.

This page covers how to add UDFs to DataFusion. In particular, it covers how to add Scalar, Window, and Aggregate UDFs.

| UDF Type       | Description                                                                                                | Example(s)                            |
| -------------- | ---------------------------------------------------------------------------------------------------------- | ------------------------------------- |
| Scalar         | A function that takes a row of data and returns a single value.                                            | [simple_udf.rs] / [advanced_udf.rs]   |
| Window         | A function that takes a row of data and returns a single value, but also has access to the rows around it. | [simple_udwf.rs] / [advanced_udwf.rs] |
| Aggregate      | A function that takes a group of rows and returns a single value.                                          | [simple_udaf.rs] / [advanced_udaf.rs] |
| Table          | A function that takes parameters and returns a `TableProvider` to be used in an query plan.                | [simple_udtf.rs]                      |
| Scalar (async) | A scalar function for performing `async` operations (such as network or I/O calls) within the UDF.         | [async_udf.rs]                        |

[simple_udf.rs]: https://github.com/apache/datafusion/blob/main/datafusion-examples/examples/simple_udf.rs
[advanced_udf.rs]: https://github.com/apache/datafusion/blob/main/datafusion-examples/examples/advanced_udf.rs
[simple_udwf.rs]: https://github.com/apache/datafusion/blob/main/datafusion-examples/examples/simple_udwf.rs
[advanced_udwf.rs]: https://github.com/apache/datafusion/blob/main/datafusion-examples/examples/advanced_udwf.rs
[simple_udaf.rs]: https://github.com/apache/datafusion/blob/main/datafusion-examples/examples/simple_udaf.rs
[advanced_udaf.rs]: https://github.com/apache/datafusion/blob/main/datafusion-examples/examples/advanced_udaf.rs
[simple_udtf.rs]: https://github.com/apache/datafusion/blob/main/datafusion-examples/examples/simple_udtf.rs
[async_udf.rs]: https://github.com/apache/datafusion/blob/main/datafusion-examples/examples/async_udf.rs

First we'll talk about adding an Scalar UDF end-to-end, then we'll talk about the differences between the different
types of UDFs.

## Adding a Scalar UDF

A Scalar UDF is a function that takes a row of data and returns a single value. To achieve good performance,
such functions are "vectorized" in DataFusion, meaning they get one or more Arrow Arrays as input and produce
an Arrow Array with the same number of rows as output.

To create a Scalar UDF, you

1. Implement the `ScalarUDFImpl` trait to tell DataFusion about your function such as what types of arguments it takes
   and how to calculate the results.
2. Create a `ScalarUDF` and register it with `SessionContext::register_udf` so it can be invoked by name.

In the following example, we will add a function takes a single i64 and returns a single i64 with 1 added to it:

For brevity, we'll skip some error handling.
For production code, you may want to check, for example, that `args.len()` matches the expected number of arguments.

### Adding by `impl ScalarUDFImpl`

This a lower level API with more functionality but is more complex, also documented in [`advanced_udf.rs`].

```rust
use std::sync::Arc;
use std::any::Any;
use std::sync::LazyLock;
use arrow::datatypes::DataType;
use datafusion_common::cast::as_int64_array;
use datafusion_common::{DataFusionError, plan_err, Result};
use datafusion_expr::{col, ColumnarValue, ScalarFunctionArgs, Signature, Volatility};
use datafusion::arrow::array::{ArrayRef, Int64Array};
use datafusion_expr::{ScalarUDFImpl, ScalarUDF};
use datafusion_macros::user_doc;
use datafusion_doc::Documentation;

/// This struct for a simple UDF that adds one to an int32
#[user_doc(
    doc_section(label = "Math Functions"),
    description = "Add one udf",
    syntax_example = "add_one(1)"
)]
#[derive(Debug)]
struct AddOne {
  signature: Signature,
}

impl AddOne {
  fn new() -> Self {
    Self {
      signature: Signature::uniform(1, vec![DataType::Int32], Volatility::Immutable),
     }
  }
}

/// Implement the ScalarUDFImpl trait for AddOne
impl ScalarUDFImpl for AddOne {
   fn as_any(&self) -> &dyn Any { self }
   fn name(&self) -> &str { "add_one" }
   fn signature(&self) -> &Signature { &self.signature }
   fn return_type(&self, args: &[DataType]) -> Result<DataType> {
     if !matches!(args.get(0), Some(&DataType::Int32)) {
       return plan_err!("add_one only accepts Int32 arguments");
     }
     Ok(DataType::Int32)
   }
   // The actual implementation would add one to the argument
   fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        let args = ColumnarValue::values_to_arrays(&args.args)?;
        let i64s = as_int64_array(&args[0])?;

        let new_array = i64s
            .iter()
            .map(|array_elem| array_elem.map(|value| value + 1))
            .collect::<Int64Array>();

        Ok(ColumnarValue::from(Arc::new(new_array) as ArrayRef))
   }
   fn documentation(&self) -> Option<&Documentation> {
       self.doc()
    }
}
```

We now need to register the function with DataFusion so that it can be used in the context of a query.

```rust
# use std::sync::Arc;
# use std::any::Any;
# use std::sync::LazyLock;
# use arrow::datatypes::DataType;
# use datafusion_common::cast::as_int64_array;
# use datafusion_common::{DataFusionError, plan_err, Result};
# use datafusion_expr::{col, ColumnarValue, ScalarFunctionArgs, Signature, Volatility};
# use datafusion::arrow::array::{ArrayRef, Int64Array};
# use datafusion_expr::{ScalarUDFImpl, ScalarUDF};
# use datafusion_macros::user_doc;
# use datafusion_doc::Documentation;
#
# /// This struct for a simple UDF that adds one to an int32
# #[user_doc(
#     doc_section(label = "Math Functions"),
#     description = "Add one udf",
#     syntax_example = "add_one(1)"
# )]
# #[derive(Debug)]
# struct AddOne {
#   signature: Signature,
# }
#
# impl AddOne {
#   fn new() -> Self {
#     Self {
#       signature: Signature::uniform(1, vec![DataType::Int32], Volatility::Immutable),
#      }
#   }
# }
#
# /// Implement the ScalarUDFImpl trait for AddOne
# impl ScalarUDFImpl for AddOne {
#    fn as_any(&self) -> &dyn Any { self }
#    fn name(&self) -> &str { "add_one" }
#    fn signature(&self) -> &Signature { &self.signature }
#    fn return_type(&self, args: &[DataType]) -> Result<DataType> {
#      if !matches!(args.get(0), Some(&DataType::Int32)) {
#        return plan_err!("add_one only accepts Int32 arguments");
#      }
#      Ok(DataType::Int32)
#    }
#    // The actual implementation would add one to the argument
#    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
#         let args = ColumnarValue::values_to_arrays(&args.args)?;
#         let i64s = as_int64_array(&args[0])?;
#
#         let new_array = i64s
#             .iter()
#             .map(|array_elem| array_elem.map(|value| value + 1))
#             .collect::<Int64Array>();
#
#         Ok(ColumnarValue::from(Arc::new(new_array) as ArrayRef))
#    }
#    fn documentation(&self) -> Option<&Documentation> {
#        self.doc()
#     }
# }
use datafusion::execution::context::SessionContext;

// Create a new ScalarUDF from the implementation
let add_one = ScalarUDF::from(AddOne::new());

// Call the function `add_one(col)`
let expr = add_one.call(vec![col("a")]);

// register the UDF with the context so it can be invoked by name and from SQL
let mut ctx = SessionContext::new();
ctx.register_udf(add_one.clone());
```

### Adding a Scalar UDF by [`create_udf`]

There is a an older, more concise, but also more limited API [`create_udf`] available as well

#### Adding a Scalar UDF

```rust
use std::sync::Arc;
use datafusion::arrow::array::{ArrayRef, Int64Array};
use datafusion::common::cast::as_int64_array;
use datafusion::common::Result;
use datafusion::logical_expr::ColumnarValue;

pub fn add_one(args: &[ColumnarValue]) -> Result<ColumnarValue> {
    // Error handling omitted for brevity
    let args = ColumnarValue::values_to_arrays(args)?;
    let i64s = as_int64_array(&args[0])?;

    let new_array = i64s
        .iter()
        .map(|array_elem| array_elem.map(|value| value + 1))
        .collect::<Int64Array>();

    Ok(ColumnarValue::from(Arc::new(new_array) as ArrayRef))
}
```

This "works" in isolation, i.e. if you have a slice of `ArrayRef`s, you can call `add_one` and it will return a new
`ArrayRef` with 1 added to each value.

```rust
# use std::sync::Arc;
# use datafusion::arrow::array::{ArrayRef, Int64Array};
# use datafusion::common::cast::as_int64_array;
# use datafusion::common::Result;
# use datafusion::logical_expr::ColumnarValue;
#
# pub fn add_one(args: &[ColumnarValue]) -> Result<ColumnarValue> {
#     // Error handling omitted for brevity
#     let args = ColumnarValue::values_to_arrays(args)?;
#     let i64s = as_int64_array(&args[0])?;
#
#     let new_array = i64s
#         .iter()
#         .map(|array_elem| array_elem.map(|value| value + 1))
#         .collect::<Int64Array>();
#
#     Ok(ColumnarValue::from(Arc::new(new_array) as ArrayRef))
# }
let input = vec![Some(1), None, Some(3)];
let input = ColumnarValue::from(Arc::new(Int64Array::from(input)) as ArrayRef);

let result = add_one(&[input]).unwrap();
let binding = result.into_array(1).unwrap();
let result = binding.as_any().downcast_ref::<Int64Array>().unwrap();

assert_eq!(result, &Int64Array::from(vec![Some(2), None, Some(4)]));
```

The challenge however is that DataFusion doesn't know about this function. We need to register it with DataFusion so
that it can be used in the context of a query.

#### Registering a Scalar UDF

To register a Scalar UDF, you need to wrap the function implementation in a [`ScalarUDF`] struct and then register it
with the `SessionContext`.
DataFusion provides the [`create_udf`] and helper functions to make this easier.

```rust
# use std::sync::Arc;
# use datafusion::arrow::array::{ArrayRef, Int64Array};
# use datafusion::common::cast::as_int64_array;
# use datafusion::common::Result;
# use datafusion::logical_expr::ColumnarValue;
#
# pub fn add_one(args: &[ColumnarValue]) -> Result<ColumnarValue> {
#     // Error handling omitted for brevity
#     let args = ColumnarValue::values_to_arrays(args)?;
#     let i64s = as_int64_array(&args[0])?;
#
#     let new_array = i64s
#         .iter()
#         .map(|array_elem| array_elem.map(|value| value + 1))
#         .collect::<Int64Array>();
#
#     Ok(ColumnarValue::from(Arc::new(new_array) as ArrayRef))
# }
use datafusion::logical_expr::{Volatility, create_udf};
use datafusion::arrow::datatypes::DataType;

let udf = create_udf(
    "add_one",
    vec![DataType::Int64],
    DataType::Int64,
    Volatility::Immutable,
    Arc::new(add_one),
);
```

A few things to note on `create_udf`:

- The first argument is the name of the function. This is the name that will be used in SQL queries.
- The second argument is a vector of `DataType`s. This is the list of argument types that the function accepts. I.e. in
  this case, the function accepts a single `Int64` argument.
- The third argument is the return type of the function. I.e. in this case, the function returns an `Int64`.
- The fourth argument is the volatility of the function. In short, this is used to determine if the function's
  performance can be optimized in some situations. In this case, the function is `Immutable` because it always returns
  the same value for the same input. A random number generator would be `Volatile` because it returns a different value
  for the same input.
- The fifth argument is the function implementation. This is the function that we defined above.

That gives us a `ScalarUDF` that we can register with the `SessionContext`:

```rust
# use std::sync::Arc;
# use datafusion::arrow::array::{ArrayRef, Int64Array};
# use datafusion::common::cast::as_int64_array;
# use datafusion::common::Result;
# use datafusion::logical_expr::ColumnarValue;
#
# pub fn add_one(args: &[ColumnarValue]) -> Result<ColumnarValue> {
#     // Error handling omitted for brevity
#     let args = ColumnarValue::values_to_arrays(args)?;
#     let i64s = as_int64_array(&args[0])?;
#
#     let new_array = i64s
#         .iter()
#         .map(|array_elem| array_elem.map(|value| value + 1))
#         .collect::<Int64Array>();
#
#     Ok(ColumnarValue::from(Arc::new(new_array) as ArrayRef))
# }
use datafusion::logical_expr::{Volatility, create_udf};
use datafusion::arrow::datatypes::DataType;
use datafusion::execution::context::SessionContext;

#[tokio::main]
async fn main() {
    let udf = create_udf(
        "add_one",
        vec![DataType::Int64],
        DataType::Int64,
        Volatility::Immutable,
        Arc::new(add_one),
    );

    let mut ctx = SessionContext::new();
    ctx.register_udf(udf);

    // At this point, you can use the `add_one` function in your query:
    let query = "SELECT add_one(1)";
    let df = ctx.sql(&query).await.unwrap();
}
```

## Adding a Async Scalar UDF

An Async Scalar UDF allows you to implement user-defined functions that support
asynchronous execution, such as performing network or I/O operations within the
UDF.

To add a Scalar Async UDF, you need to:

1. Implement the `AsyncScalarUDFImpl` trait to define your async function logic, signature, and types.
2. Wrap your implementation with `AsyncScalarUDF::new` and register it with the `SessionContext`.

### Adding by `impl AsyncScalarUDFImpl`

```rust
# use arrow::array::{ArrayIter, ArrayRef, AsArray, StringArray};
# use arrow_schema::DataType;
# use async_trait::async_trait;
# use datafusion::common::error::Result;
# use datafusion::common::{internal_err, not_impl_err};
# use datafusion::common::types::logical_string;
# use datafusion::config::ConfigOptions;
# use datafusion_expr::ScalarUDFImpl;
# use datafusion::logical_expr::async_udf::AsyncScalarUDFImpl;
# use datafusion::logical_expr::{
#     ColumnarValue, Signature, TypeSignature, TypeSignatureClass, Volatility, ScalarFunctionArgs
# };
# use datafusion::logical_expr_common::signature::Coercion;
# use std::any::Any;
# use std::sync::Arc;

#[derive(Debug)]
pub struct AsyncUpper {
    signature: Signature,
}

impl Default for AsyncUpper {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncUpper {
    pub fn new() -> Self {
        Self {
            signature: Signature::new(
                TypeSignature::Coercible(vec![Coercion::Exact {
                    desired_type: TypeSignatureClass::Native(logical_string()),
                }]),
                Volatility::Volatile,
            ),
        }
    }
}

/// Implement the normal ScalarUDFImpl trait for AsyncUpper
#[async_trait]
impl ScalarUDFImpl for AsyncUpper {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> &str {
        "async_upper"
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Utf8)
    }

    // Note the normal invoke_with_args method is not called for Async UDFs
    fn invoke_with_args(
        &self,
        _args: ScalarFunctionArgs,
    ) -> Result<ColumnarValue> {
        not_impl_err!("AsyncUpper can only be called from async contexts")
    }
}

/// The actual implementation of the async UDF
#[async_trait]
impl AsyncScalarUDFImpl for AsyncUpper {
    fn ideal_batch_size(&self) -> Option<usize> {
        Some(10)
    }

    /// This method is called to execute the async UDF and is similar
    /// to the normal `invoke_with_args` except it returns an `ArrayRef`
    /// instead of `ColumnarValue` and is `async`.
    async fn invoke_async_with_args(
        &self,
        args: ScalarFunctionArgs,
        _option: &ConfigOptions,
    ) -> Result<ArrayRef> {
        let value = &args.args[0];
        // This function simply implements a simple string to uppercase conversion
        // but can be used for any async operation such as network calls.
        let result = match value {
            ColumnarValue::Array(array) => {
                let string_array = array.as_string::<i32>();
                let iter = ArrayIter::new(string_array);
                let result = iter
                    .map(|string| string.map(|s| s.to_uppercase()))
                    .collect::<StringArray>();
                Arc::new(result) as ArrayRef
            }
            _ => return internal_err!("Expected a string argument, got {:?}", value),
        };
        Ok(result)
    }
}
```

We can now transfer the async UDF into the normal scalar using `into_scalar_udf` to register the function with DataFusion so that it can be used in the context of a query.

```rust
# use arrow::array::{ArrayIter, ArrayRef, AsArray, StringArray};
# use arrow_schema::DataType;
# use async_trait::async_trait;
# use datafusion::common::error::Result;
# use datafusion::common::{internal_err, not_impl_err};
# use datafusion::common::types::logical_string;
# use datafusion::config::ConfigOptions;
# use datafusion_expr::ScalarUDFImpl;
# use datafusion::logical_expr::async_udf::AsyncScalarUDFImpl;
# use datafusion::logical_expr::{
#     ColumnarValue, Signature, TypeSignature, TypeSignatureClass, Volatility, ScalarFunctionArgs
# };
# use datafusion::logical_expr_common::signature::Coercion;
# use log::trace;
# use std::any::Any;
# use std::sync::Arc;
#
# #[derive(Debug)]
# pub struct AsyncUpper {
#     signature: Signature,
# }
#
# impl Default for AsyncUpper {
#     fn default() -> Self {
#         Self::new()
#     }
# }
#
# impl AsyncUpper {
#     pub fn new() -> Self {
#         Self {
#             signature: Signature::new(
#                 TypeSignature::Coercible(vec![Coercion::Exact {
#                     desired_type: TypeSignatureClass::Native(logical_string()),
#                 }]),
#                 Volatility::Volatile,
#             ),
#         }
#     }
# }
#
# #[async_trait]
# impl ScalarUDFImpl for AsyncUpper {
#     fn as_any(&self) -> &dyn Any {
#         self
#     }
#
#     fn name(&self) -> &str {
#         "async_upper"
#     }
#
#     fn signature(&self) -> &Signature {
#         &self.signature
#     }
#
#     fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
#         Ok(DataType::Utf8)
#     }
#
#     fn invoke_with_args(
#        &self,
#        _args: ScalarFunctionArgs,
#     ) -> Result<ColumnarValue> {
#         not_impl_err!("AsyncUpper can only be called from async contexts")
#     }
# }
#
# #[async_trait]
# impl AsyncScalarUDFImpl for AsyncUpper {
#     fn ideal_batch_size(&self) -> Option<usize> {
#         Some(10)
#     }
#
#     async fn invoke_async_with_args(
#         &self,
#         args: ScalarFunctionArgs,
#         _option: &ConfigOptions,
#     ) -> Result<ArrayRef> {
#         trace!("Invoking async_upper with args: {:?}", args);
#         let value = &args.args[0];
#         let result = match value {
#             ColumnarValue::Array(array) => {
#                 let string_array = array.as_string::<i32>();
#                 let iter = ArrayIter::new(string_array);
#                 let result = iter
#                     .map(|string| string.map(|s| s.to_uppercase()))
#                     .collect::<StringArray>();
#                 Arc::new(result) as ArrayRef
#             }
#             _ => return internal_err!("Expected a string argument, got {:?}", value),
#         };
#         Ok(result)
#     }
# }
use datafusion::execution::context::SessionContext;
use datafusion::logical_expr::async_udf::AsyncScalarUDF;

let async_upper = AsyncUpper::new();
let udf = AsyncScalarUDF::new(Arc::new(async_upper));
let mut ctx = SessionContext::new();
ctx.register_udf(udf.into_scalar_udf());
```

After registration, you can use these async UDFs directly in SQL queries, for example:

```sql
SELECT async_upper('datafusion');
```

For async UDF implementation details, see [`async_udf.rs`](https://github.com/apache/datafusion/blob/main/datafusion-examples/examples/async_udf.rs).

[`scalarudf`]: https://docs.rs/datafusion/latest/datafusion/logical_expr/struct.ScalarUDF.html
[`create_udf`]: https://docs.rs/datafusion/latest/datafusion/logical_expr/fn.create_udf.html
[`process_scalar_func_inputs`]: https://docs.rs/datafusion/latest/datafusion/physical_expr/functions/fn.process_scalar_func_inputs.html
[`advanced_udf.rs`]: https://github.com/apache/datafusion/blob/main/datafusion-examples/examples/advanced_udf.rs

## Adding a Window UDF

Scalar UDFs are functions that take a row of data and return a single value. Window UDFs are similar, but they also have
access to the rows around them. Access to the proximal rows is helpful, but adds some complexity to the implementation.

For example, we will declare a user defined window function that computes a moving average.

```rust
use datafusion::arrow::{array::{ArrayRef, Float64Array, AsArray}, datatypes::Float64Type};
use datafusion::logical_expr::{PartitionEvaluator};
use datafusion::common::ScalarValue;
use datafusion::error::Result;
/// This implements the lowest level evaluation for a window function
///
/// It handles calculating the value of the window function for each
/// distinct values of `PARTITION BY`
#[derive(Clone, Debug)]
struct MyPartitionEvaluator {}

impl MyPartitionEvaluator {
    fn new() -> Self {
        Self {}
    }
}

/// Different evaluation methods are called depending on the various
/// settings of WindowUDF. This example uses the simplest and most
/// general, `evaluate`. See `PartitionEvaluator` for the other more
/// advanced uses.
impl PartitionEvaluator for MyPartitionEvaluator {
    /// Tell DataFusion the window function varies based on the value
    /// of the window frame.
    fn uses_window_frame(&self) -> bool {
        true
    }

    /// This function is called once per input row.
    ///
    /// `range`specifies which indexes of `values` should be
    /// considered for the calculation.
    ///
    /// Note this is the SLOWEST, but simplest, way to evaluate a
    /// window function. It is much faster to implement
    /// evaluate_all or evaluate_all_with_rank, if possible
    fn evaluate(
        &mut self,
        values: &[ArrayRef],
        range: &std::ops::Range<usize>,
    ) -> Result<ScalarValue> {
        // Again, the input argument is an array of floating
        // point numbers to calculate a moving average
        let arr: &Float64Array = values[0].as_ref().as_primitive::<Float64Type>();

        let range_len = range.end - range.start;

        // our smoothing function will average all the values in the
        let output = if range_len > 0 {
            let sum: f64 = arr.values().iter().skip(range.start).take(range_len).sum();
            Some(sum / range_len as f64)
        } else {
            None
        };

        Ok(ScalarValue::Float64(output))
    }
}

/// Create a `PartitionEvaluator` to evaluate this function on a new
/// partition.
fn make_partition_evaluator() -> Result<Box<dyn PartitionEvaluator>> {
    Ok(Box::new(MyPartitionEvaluator::new()))
}
```

### Registering a Window UDF

To register a Window UDF, you need to wrap the function implementation in a [`WindowUDF`] struct and then register it
with the `SessionContext`. DataFusion provides the [`create_udwf`] helper functions to make this easier.
There is a lower level API with more functionality but is more complex, that is documented in [`advanced_udwf.rs`].

```rust
# use datafusion::arrow::{array::{ArrayRef, Float64Array, AsArray}, datatypes::Float64Type};
# use datafusion::logical_expr::{PartitionEvaluator};
# use datafusion::common::ScalarValue;
# use datafusion::error::Result;
#
# #[derive(Clone, Debug)]
# struct MyPartitionEvaluator {}
#
# impl MyPartitionEvaluator {
#     fn new() -> Self {
#         Self {}
#     }
# }
#
# impl PartitionEvaluator for MyPartitionEvaluator {
#     fn uses_window_frame(&self) -> bool {
#         true
#     }
#
#     fn evaluate(
#         &mut self,
#         values: &[ArrayRef],
#         range: &std::ops::Range<usize>,
#     ) -> Result<ScalarValue> {
#         // Again, the input argument is an array of floating
#         // point numbers to calculate a moving average
#         let arr: &Float64Array = values[0].as_ref().as_primitive::<Float64Type>();
#
#         let range_len = range.end - range.start;
#
#         // our smoothing function will average all the values in the
#         let output = if range_len > 0 {
#             let sum: f64 = arr.values().iter().skip(range.start).take(range_len).sum();
#             Some(sum / range_len as f64)
#         } else {
#             None
#         };
#
#         Ok(ScalarValue::Float64(output))
#     }
# }
# fn make_partition_evaluator() -> Result<Box<dyn PartitionEvaluator>> {
#     Ok(Box::new(MyPartitionEvaluator::new()))
# }
use datafusion::logical_expr::{Volatility, create_udwf};
use datafusion::arrow::datatypes::DataType;
use std::sync::Arc;

// here is where we define the UDWF. We also declare its signature:
let smooth_it = create_udwf(
    "smooth_it",
    DataType::Float64,
    Arc::new(DataType::Float64),
    Volatility::Immutable,
    Arc::new(make_partition_evaluator),
);
```

[`windowudf`]: https://docs.rs/datafusion/latest/datafusion/logical_expr/struct.WindowUDF.html
[`create_udwf`]: https://docs.rs/datafusion/latest/datafusion/logical_expr/fn.create_udwf.html
[`advanced_udwf.rs`]: https://github.com/apache/datafusion/blob/main/datafusion-examples/examples/advanced_udwf.rs

The `create_udwf` has five arguments to check:

- The first argument is the name of the function. This is the name that will be used in SQL queries.
- **The second argument** is the `DataType` of input array (attention: this is not a list of arrays). I.e. in this case,
  the function accepts `Float64` as argument.
- The third argument is the return type of the function. I.e. in this case, the function returns an `Float64`.
- The fourth argument is the volatility of the function. In short, this is used to determine if the function's
  performance can be optimized in some situations. In this case, the function is `Immutable` because it always returns
  the same value for the same input. A random number generator would be `Volatile` because it returns a different value
  for the same input.
- **The fifth argument** is the function implementation. This is the function that we defined above.

That gives us a `WindowUDF` that we can register with the `SessionContext`:

```rust
# use datafusion::arrow::{array::{ArrayRef, Float64Array, AsArray}, datatypes::Float64Type};
# use datafusion::logical_expr::{PartitionEvaluator};
# use datafusion::common::ScalarValue;
# use datafusion::error::Result;
#
# #[derive(Clone, Debug)]
# struct MyPartitionEvaluator {}
#
# impl MyPartitionEvaluator {
#     fn new() -> Self {
#         Self {}
#     }
# }
#
# impl PartitionEvaluator for MyPartitionEvaluator {
#     fn uses_window_frame(&self) -> bool {
#         true
#     }
#
#     fn evaluate(
#         &mut self,
#         values: &[ArrayRef],
#         range: &std::ops::Range<usize>,
#     ) -> Result<ScalarValue> {
#         // Again, the input argument is an array of floating
#         // point numbers to calculate a moving average
#         let arr: &Float64Array = values[0].as_ref().as_primitive::<Float64Type>();
#
#         let range_len = range.end - range.start;
#
#         // our smoothing function will average all the values in the
#         let output = if range_len > 0 {
#             let sum: f64 = arr.values().iter().skip(range.start).take(range_len).sum();
#             Some(sum / range_len as f64)
#         } else {
#             None
#         };
#
#         Ok(ScalarValue::Float64(output))
#     }
# }
# fn make_partition_evaluator() -> Result<Box<dyn PartitionEvaluator>> {
#     Ok(Box::new(MyPartitionEvaluator::new()))
# }
# use datafusion::logical_expr::{Volatility, create_udwf};
# use datafusion::arrow::datatypes::DataType;
# use std::sync::Arc;
#
# // here is where we define the UDWF. We also declare its signature:
# let smooth_it = create_udwf(
#     "smooth_it",
#     DataType::Float64,
#     Arc::new(DataType::Float64),
#     Volatility::Immutable,
#     Arc::new(make_partition_evaluator),
# );
use datafusion::execution::context::SessionContext;

let ctx = SessionContext::new();

ctx.register_udwf(smooth_it);
```

At this point, you can use the `smooth_it` function in your query:

For example, if we have a [`cars.csv`](https://github.com/apache/datafusion/blob/main/datafusion/core/tests/data/cars.csv) whose contents like

```csv
car,speed,time
red,20.0,1996-04-12T12:05:03.000000000
red,20.3,1996-04-12T12:05:04.000000000
green,10.0,1996-04-12T12:05:03.000000000
green,10.3,1996-04-12T12:05:04.000000000
...
```

Then, we can query like below:

```rust
# use datafusion::arrow::{array::{ArrayRef, Float64Array, AsArray}, datatypes::Float64Type};
# use datafusion::logical_expr::{PartitionEvaluator};
# use datafusion::common::ScalarValue;
# use datafusion::error::Result;
#
# #[derive(Clone, Debug)]
# struct MyPartitionEvaluator {}
#
# impl MyPartitionEvaluator {
#     fn new() -> Self {
#         Self {}
#     }
# }
#
# impl PartitionEvaluator for MyPartitionEvaluator {
#     fn uses_window_frame(&self) -> bool {
#         true
#     }
#
#     fn evaluate(
#         &mut self,
#         values: &[ArrayRef],
#         range: &std::ops::Range<usize>,
#     ) -> Result<ScalarValue> {
#         // Again, the input argument is an array of floating
#         // point numbers to calculate a moving average
#         let arr: &Float64Array = values[0].as_ref().as_primitive::<Float64Type>();
#
#         let range_len = range.end - range.start;
#
#         // our smoothing function will average all the values in the
#         let output = if range_len > 0 {
#             let sum: f64 = arr.values().iter().skip(range.start).take(range_len).sum();
#             Some(sum / range_len as f64)
#         } else {
#             None
#         };
#
#         Ok(ScalarValue::Float64(output))
#     }
# }
# fn make_partition_evaluator() -> Result<Box<dyn PartitionEvaluator>> {
#     Ok(Box::new(MyPartitionEvaluator::new()))
# }
# use datafusion::logical_expr::{Volatility, create_udwf};
# use datafusion::arrow::datatypes::DataType;
# use std::sync::Arc;
# use datafusion::execution::context::SessionContext;

use datafusion::datasource::file_format::options::CsvReadOptions;

#[tokio::main]
async fn main() -> Result<()> {

    let ctx = SessionContext::new();

    let smooth_it = create_udwf(
        "smooth_it",
        DataType::Float64,
        Arc::new(DataType::Float64),
        Volatility::Immutable,
        Arc::new(make_partition_evaluator),
    );
    ctx.register_udwf(smooth_it);

    // register csv table first
    let csv_path = "../../datafusion/core/tests/data/cars.csv".to_string();
    ctx.register_csv("cars", &csv_path, CsvReadOptions::default().has_header(true)).await?;

    // do query with smooth_it
    let df = ctx
        .sql(r#"
            SELECT
                car,
                speed,
                smooth_it(speed) OVER (PARTITION BY car ORDER BY time) as smooth_speed,
                time
            FROM cars
            ORDER BY car
        "#)
        .await?;

    // print the results
    df.show().await?;
    Ok(())
}
```

The output will be like:

```text
+-------+-------+--------------------+---------------------+
| car   | speed | smooth_speed       | time                |
+-------+-------+--------------------+---------------------+
| green | 10.0  | 10.0               | 1996-04-12T12:05:03 |
| green | 10.3  | 10.15              | 1996-04-12T12:05:04 |
| green | 10.4  | 10.233333333333334 | 1996-04-12T12:05:05 |
| green | 10.5  | 10.3               | 1996-04-12T12:05:06 |
| green | 11.0  | 10.440000000000001 | 1996-04-12T12:05:07 |
| green | 12.0  | 10.700000000000001 | 1996-04-12T12:05:08 |
| green | 14.0  | 11.171428571428573 | 1996-04-12T12:05:09 |
| green | 15.0  | 11.65              | 1996-04-12T12:05:10 |
| green | 15.1  | 12.033333333333333 | 1996-04-12T12:05:11 |
| green | 15.2  | 12.35              | 1996-04-12T12:05:12 |
| green | 8.0   | 11.954545454545455 | 1996-04-12T12:05:13 |
| green | 2.0   | 11.125             | 1996-04-12T12:05:14 |
| red   | 20.0  | 20.0               | 1996-04-12T12:05:03 |
| red   | 20.3  | 20.15              | 1996-04-12T12:05:04 |
...
```

## Adding an Aggregate UDF

Aggregate UDFs are functions that take a group of rows and return a single value. These are akin to SQL's `SUM` or
`COUNT` functions.

For example, we will declare a single-type, single return type UDAF that computes the geometric mean.

```rust

use datafusion::arrow::array::ArrayRef;
use datafusion::scalar::ScalarValue;
use datafusion::{error::Result, physical_plan::Accumulator};

/// A UDAF has state across multiple rows, and thus we require a `struct` with that state.
#[derive(Debug)]
struct GeometricMean {
    n: u32,
    prod: f64,
}

impl GeometricMean {
    // how the struct is initialized
    pub fn new() -> Self {
        GeometricMean { n: 0, prod: 1.0 }
    }
}

// UDAFs are built using the trait `Accumulator`, that offers DataFusion the necessary functions
// to use them.
impl Accumulator for GeometricMean {
    // This function serializes our state to `ScalarValue`, which DataFusion uses
    // to pass this state between execution stages.
    // Note that this can be arbitrary data.
    fn state(&mut self) -> Result<Vec<ScalarValue>> {
        Ok(vec![
            ScalarValue::from(self.prod),
            ScalarValue::from(self.n),
        ])
    }

    // DataFusion expects this function to return the final value of this aggregator.
    // in this case, this is the formula of the geometric mean
    fn evaluate(&mut self) -> Result<ScalarValue> {
        let value = self.prod.powf(1.0 / self.n as f64);
        Ok(ScalarValue::from(value))
    }

    // DataFusion calls this function to update the accumulator's state for a batch
    // of inputs rows. In this case the product is updated with values from the first column
    // and the count is updated based on the row count
    fn update_batch(&mut self, values: &[ArrayRef]) -> Result<()> {
        if values.is_empty() {
            return Ok(());
        }
        let arr = &values[0];
        (0..arr.len()).try_for_each(|index| {
            let v = ScalarValue::try_from_array(arr, index)?;

            if let ScalarValue::Float64(Some(value)) = v {
                self.prod *= value;
                self.n += 1;
            } else {
                unreachable!("")
            }
            Ok(())
        })
    }

    // Optimization hint: this trait also supports `update_batch` and `merge_batch`,
    // that can be used to perform these operations on arrays instead of single values.
    fn merge_batch(&mut self, states: &[ArrayRef]) -> Result<()> {
        if states.is_empty() {
            return Ok(());
        }
        let arr = &states[0];
        (0..arr.len()).try_for_each(|index| {
            let v = states
                .iter()
                .map(|array| ScalarValue::try_from_array(array, index))
                .collect::<Result<Vec<_>>>()?;
            if let (ScalarValue::Float64(Some(prod)), ScalarValue::UInt32(Some(n))) = (&v[0], &v[1])
            {
                self.prod *= prod;
                self.n += n;
            } else {
                unreachable!("")
            }
            Ok(())
        })
    }

    fn size(&self) -> usize {
        std::mem::size_of_val(self)
    }
}
```

### Registering an Aggregate UDF

To register a Aggregate UDF, you need to wrap the function implementation in a [`AggregateUDF`] struct and then register
it with the `SessionContext`. DataFusion provides the [`create_udaf`] helper functions to make this easier.
There is a lower level API with more functionality but is more complex, that is documented in [`advanced_udaf.rs`].

```rust
# use datafusion::arrow::array::ArrayRef;
# use datafusion::scalar::ScalarValue;
# use datafusion::{error::Result, physical_plan::Accumulator};
#
# #[derive(Debug)]
# struct GeometricMean {
#     n: u32,
#     prod: f64,
# }
#
# impl GeometricMean {
#     pub fn new() -> Self {
#         GeometricMean { n: 0, prod: 1.0 }
#     }
# }
#
# impl Accumulator for GeometricMean {
#     fn state(&mut self) -> Result<Vec<ScalarValue>> {
#         Ok(vec![
#             ScalarValue::from(self.prod),
#             ScalarValue::from(self.n),
#         ])
#     }
#
#     fn evaluate(&mut self) -> Result<ScalarValue> {
#         let value = self.prod.powf(1.0 / self.n as f64);
#         Ok(ScalarValue::from(value))
#     }
#
#     fn update_batch(&mut self, values: &[ArrayRef]) -> Result<()> {
#         if values.is_empty() {
#             return Ok(());
#         }
#         let arr = &values[0];
#         (0..arr.len()).try_for_each(|index| {
#             let v = ScalarValue::try_from_array(arr, index)?;
#
#             if let ScalarValue::Float64(Some(value)) = v {
#                 self.prod *= value;
#                 self.n += 1;
#             } else {
#                 unreachable!("")
#             }
#             Ok(())
#         })
#     }
#
#     fn merge_batch(&mut self, states: &[ArrayRef]) -> Result<()> {
#         if states.is_empty() {
#             return Ok(());
#         }
#         let arr = &states[0];
#         (0..arr.len()).try_for_each(|index| {
#             let v = states
#                 .iter()
#                 .map(|array| ScalarValue::try_from_array(array, index))
#                 .collect::<Result<Vec<_>>>()?;
#             if let (ScalarValue::Float64(Some(prod)), ScalarValue::UInt32(Some(n))) = (&v[0], &v[1])
#             {
#                 self.prod *= prod;
#                 self.n += n;
#             } else {
#                 unreachable!("")
#             }
#             Ok(())
#         })
#     }
#
#     fn size(&self) -> usize {
#         std::mem::size_of_val(self)
#     }
# }

use datafusion::logical_expr::{Volatility, create_udaf};
use datafusion::arrow::datatypes::DataType;
use std::sync::Arc;

// here is where we define the UDAF. We also declare its signature:
let geometric_mean = create_udaf(
    // the name; used to represent it in plan descriptions and in the registry, to use in SQL.
    "geo_mean",
    // the input type; DataFusion guarantees that the first entry of `values` in `update` has this type.
    vec![DataType::Float64],
    // the return type; DataFusion expects this to match the type returned by `evaluate`.
    Arc::new(DataType::Float64),
    Volatility::Immutable,
    // This is the accumulator factory; DataFusion uses it to create new accumulators.
    Arc::new( | _ | Ok(Box::new(GeometricMean::new()))),
    // This is the description of the state. `state()` must match the types here.
    Arc::new(vec![DataType::Float64, DataType::UInt32]),
);
```

The `create_udaf` has six arguments to check:

- The first argument is the name of the function. This is the name that will be used in SQL queries.
- The second argument is a vector of `DataType`s. This is the list of argument types that the function accepts. I.e. in
  this case, the function accepts a single `Float64` argument.
- The third argument is the return type of the function. I.e. in this case, the function returns an `Int64`.
- The fourth argument is the volatility of the function. In short, this is used to determine if the function's
  performance can be optimized in some situations. In this case, the function is `Immutable` because it always returns
  the same value for the same input. A random number generator would be `Volatile` because it returns a different value
  for the same input.
- The fifth argument is the function implementation. This is the function that we defined above.
- The sixth argument is the description of the state, which will by passed between execution stages.

```rust

# use datafusion::arrow::array::ArrayRef;
# use datafusion::scalar::ScalarValue;
# use datafusion::{error::Result, physical_plan::Accumulator};
#
# #[derive(Debug)]
# struct GeometricMean {
#     n: u32,
#     prod: f64,
# }
#
# impl GeometricMean {
#     pub fn new() -> Self {
#         GeometricMean { n: 0, prod: 1.0 }
#     }
# }
#
# impl Accumulator for GeometricMean {
#     fn state(&mut self) -> Result<Vec<ScalarValue>> {
#         Ok(vec![
#             ScalarValue::from(self.prod),
#             ScalarValue::from(self.n),
#         ])
#     }
#
#     fn evaluate(&mut self) -> Result<ScalarValue> {
#         let value = self.prod.powf(1.0 / self.n as f64);
#         Ok(ScalarValue::from(value))
#     }
#
#     fn update_batch(&mut self, values: &[ArrayRef]) -> Result<()> {
#         if values.is_empty() {
#             return Ok(());
#         }
#         let arr = &values[0];
#         (0..arr.len()).try_for_each(|index| {
#             let v = ScalarValue::try_from_array(arr, index)?;
#
#             if let ScalarValue::Float64(Some(value)) = v {
#                 self.prod *= value;
#                 self.n += 1;
#             } else {
#                 unreachable!("")
#             }
#             Ok(())
#         })
#     }
#
#     fn merge_batch(&mut self, states: &[ArrayRef]) -> Result<()> {
#         if states.is_empty() {
#             return Ok(());
#         }
#         let arr = &states[0];
#         (0..arr.len()).try_for_each(|index| {
#             let v = states
#                 .iter()
#                 .map(|array| ScalarValue::try_from_array(array, index))
#                 .collect::<Result<Vec<_>>>()?;
#             if let (ScalarValue::Float64(Some(prod)), ScalarValue::UInt32(Some(n))) = (&v[0], &v[1])
#             {
#                 self.prod *= prod;
#                 self.n += n;
#             } else {
#                 unreachable!("")
#             }
#             Ok(())
#         })
#     }
#
#     fn size(&self) -> usize {
#         std::mem::size_of_val(self)
#     }
# }

use datafusion::logical_expr::{Volatility, create_udaf};
use datafusion::arrow::datatypes::DataType;
use std::sync::Arc;
use datafusion::execution::context::SessionContext;
use datafusion::datasource::file_format::options::CsvReadOptions;

#[tokio::main]
async fn main() -> Result<()> {
    let geometric_mean = create_udaf(
        "geo_mean",
        vec![DataType::Float64],
        Arc::new(DataType::Float64),
        Volatility::Immutable,
        Arc::new( | _ | Ok(Box::new(GeometricMean::new()))),
        Arc::new(vec![DataType::Float64, DataType::UInt32]),
    );

    // That gives us a `AggregateUDF` that we can register with the `SessionContext`:
    use datafusion::execution::context::SessionContext;

    let ctx = SessionContext::new();
    ctx.register_udaf(geometric_mean);

    // register csv table first
    let csv_path = "../../datafusion/core/tests/data/cars.csv".to_string();
    ctx.register_csv("cars", &csv_path, CsvReadOptions::default().has_header(true)).await?;

    // Then, we can query like below:
    let df = ctx.sql("SELECT geo_mean(speed) FROM cars").await?;
    Ok(())
}

```

[`aggregateudf`]: https://docs.rs/datafusion/latest/datafusion/logical_expr/struct.AggregateUDF.html
[`create_udaf`]: https://docs.rs/datafusion/latest/datafusion/logical_expr/fn.create_udaf.html
[`advanced_udaf.rs`]: https://github.com/apache/datafusion/blob/main/datafusion-examples/examples/advanced_udaf.rs

## Adding a User-Defined Table Function

A User-Defined Table Function (UDTF) is a function that takes parameters and returns a `TableProvider`.

Because we're returning a `TableProvider`, in this example we'll use the `MemTable` data source to represent a table.
This is a simple struct that holds a set of RecordBatches in memory and treats them as a table. In your case, this would
be replaced with your own struct that implements `TableProvider`.

While this is a simple example for illustrative purposes, UDTFs have a lot of potential use cases. And can be
particularly useful for reading data from external sources and interactive analysis. For example, see the [example][4]
for a working example that reads from a CSV file. As another example, you could use the built-in UDTF `parquet_metadata`
in the CLI to read the metadata from a Parquet file.

```console
> select filename, row_group_id, row_group_num_rows, row_group_bytes, stats_min, stats_max from parquet_metadata('./benchmarks/data/hits.parquet') where  column_id = 17 limit 10;
+--------------------------------+--------------+--------------------+-----------------+-----------+-----------+
| filename                       | row_group_id | row_group_num_rows | row_group_bytes | stats_min | stats_max |
+--------------------------------+--------------+--------------------+-----------------+-----------+-----------+
| ./benchmarks/data/hits.parquet | 0            | 450560             | 188921521       | 0         | 73256     |
| ./benchmarks/data/hits.parquet | 1            | 612174             | 210338885       | 0         | 109827    |
| ./benchmarks/data/hits.parquet | 2            | 344064             | 161242466       | 0         | 122484    |
| ./benchmarks/data/hits.parquet | 3            | 606208             | 235549898       | 0         | 121073    |
| ./benchmarks/data/hits.parquet | 4            | 335872             | 137103898       | 0         | 108996    |
| ./benchmarks/data/hits.parquet | 5            | 311296             | 145453612       | 0         | 108996    |
| ./benchmarks/data/hits.parquet | 6            | 303104             | 138833963       | 0         | 108996    |
| ./benchmarks/data/hits.parquet | 7            | 303104             | 191140113       | 0         | 73256     |
| ./benchmarks/data/hits.parquet | 8            | 573440             | 208038598       | 0         | 95823     |
| ./benchmarks/data/hits.parquet | 9            | 344064             | 147838157       | 0         | 73256     |
+--------------------------------+--------------+--------------------+-----------------+-----------+-----------+
```

### Writing the UDTF

The simple UDTF used here takes a single `Int64` argument and returns a table with a single column with the value of the
argument. To create a function in DataFusion, you need to implement the `TableFunctionImpl` trait. This trait has a
single method, `call`, that takes a slice of `Expr`s and returns a `Result<Arc<dyn TableProvider>>`.

In the `call` method, you parse the input `Expr`s and return a `TableProvider`. You might also want to do some
validation of the input `Expr`s, e.g. checking that the number of arguments is correct.

```rust
use std::sync::Arc;
use datafusion::common::{plan_err, ScalarValue, Result};
use datafusion::catalog::{TableFunctionImpl, TableProvider};
use datafusion::arrow::array::{ArrayRef, Int64Array};
use datafusion::datasource::memory::MemTable;
use arrow::record_batch::RecordBatch;
use arrow::datatypes::{DataType, Field, Schema};
use datafusion_expr::Expr;

/// A table function that returns a table provider with the value as a single column
#[derive(Debug)]
pub struct EchoFunction {}

impl TableFunctionImpl for EchoFunction {
    fn call(&self, exprs: &[Expr]) -> Result<Arc<dyn TableProvider>> {
        let Some(Expr::Literal(ScalarValue::Int64(Some(value)), _)) = exprs.get(0) else {
            return plan_err!("First argument must be an integer");
        };

        // Create the schema for the table
        let schema = Arc::new(Schema::new(vec![Field::new("a", DataType::Int64, false)]));

        // Create a single RecordBatch with the value as a single column
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(Int64Array::from(vec![*value]))],
        )?;

        // Create a MemTable plan that returns the RecordBatch
        let provider = MemTable::try_new(schema, vec![vec![batch]])?;

        Ok(Arc::new(provider))
    }
}
```

### Registering and Using the UDTF

With the UDTF implemented, you can register it with the `SessionContext`:

```rust
# use std::sync::Arc;
# use datafusion::common::{plan_err, ScalarValue, Result};
# use datafusion::catalog::{TableFunctionImpl, TableProvider};
# use datafusion::arrow::array::{ArrayRef, Int64Array};
# use datafusion::datasource::memory::MemTable;
# use arrow::record_batch::RecordBatch;
# use arrow::datatypes::{DataType, Field, Schema};
# use datafusion_expr::Expr;
#
# /// A table function that returns a table provider with the value as a single column
# #[derive(Debug, Default)]
# pub struct EchoFunction {}
#
# impl TableFunctionImpl for EchoFunction {
#     fn call(&self, exprs: &[Expr]) -> Result<Arc<dyn TableProvider>> {
#         let Some(Expr::Literal(ScalarValue::Int64(Some(value)), _)) = exprs.get(0) else {
#             return plan_err!("First argument must be an integer");
#         };
#
#         // Create the schema for the table
#         let schema = Arc::new(Schema::new(vec![Field::new("a", DataType::Int64, false)]));
#
#         // Create a single RecordBatch with the value as a single column
#         let batch = RecordBatch::try_new(
#             schema.clone(),
#             vec![Arc::new(Int64Array::from(vec![*value]))],
#         )?;
#
#         // Create a MemTable plan that returns the RecordBatch
#         let provider = MemTable::try_new(schema, vec![vec![batch]])?;
#
#         Ok(Arc::new(provider))
#     }
# }

use datafusion::execution::context::SessionContext;
use datafusion::arrow::util::pretty;

#[tokio::main]
async fn main() -> Result<()> {
    let ctx = SessionContext::new();

    ctx.register_udtf("echo", Arc::new(EchoFunction::default()));

    // And if all goes well, you can use it in your query:

    let results = ctx.sql("SELECT * FROM echo(1)").await?.collect().await?;
    pretty::print_batches(&results)?;
    Ok(())
}

// +---+
// | a |
// +---+
// | 1 |
// +---+
```

## Custom Expression Planning

DataFusion provides native support for common SQL operators by default such as `+`, `-`, `||`. However it does not provide support for other operators such as `@>`. To override DataFusion's default handling or support unsupported operators, developers can extend DataFusion by implementing custom expression planning, a core feature of DataFusion

### Implementing Custom Expression Planning

To extend DataFusion with support for custom operators not natively available, you need to:

1. Implement the `ExprPlanner` trait: This allows you to define custom logic for planning expressions that DataFusion doesn't natively recognize. The trait provides the necessary interface to translate SQL AST nodes into logical `Expr`.

   For detailed documentation please see: [Trait ExprPlanner](https://docs.rs/datafusion/latest/datafusion/logical_expr/planner/trait.ExprPlanner.html)

2. Register your custom planner: Integrate your implementation with DataFusion's `SessionContext` to ensure your custom planning logic is invoked during the query optimization and execution planning phase.

   For a detailed documentation see: [fn register_expr_planner](https://docs.rs/datafusion/latest/datafusion/execution/trait.FunctionRegistry.html#method.register_expr_planner)

See example below:

```rust
# use arrow::array::RecordBatch;
# use std::sync::Arc;

# use datafusion::common::{assert_batches_eq, DFSchema};
# use datafusion::error::Result;
# use datafusion::execution::FunctionRegistry;
# use datafusion::logical_expr::Operator;
# use datafusion::prelude::*;
# use datafusion::sql::sqlparser::ast::BinaryOperator;
# use datafusion_common::ScalarValue;
# use datafusion_expr::expr::Alias;
# use datafusion_expr::planner::{ExprPlanner, PlannerResult, RawBinaryExpr};
# use datafusion_expr::BinaryExpr;

# #[derive(Debug)]
# // Define the custom planner
# struct MyCustomPlanner;

// Implement ExprPlanner to add support for the `->` custom operator
impl ExprPlanner for MyCustomPlanner {
    fn plan_binary_op(
        &self,
        expr: RawBinaryExpr,
        _schema: &DFSchema,
    ) -> Result<PlannerResult<RawBinaryExpr>> {
        match &expr.op {
            // Map `->` to string concatenation
            BinaryOperator::Arrow => {
                // Rewrite `->` as a string concatenation operation
                // - `left` and `right` are the operands (e.g., 'hello' and 'world')
                // - `Operator::StringConcat` tells DataFusion to concatenate them
                Ok(PlannerResult::Planned(Expr::BinaryExpr(BinaryExpr {
                    left: Box::new(expr.left.clone()),
                    right: Box::new(expr.right.clone()),
                    op: Operator::StringConcat,
                })))
            }
            _ => Ok(PlannerResult::Original(expr)),
        }
    }
}

use datafusion::execution::context::SessionContext;
use datafusion::arrow::util::pretty;

#[tokio::main]
async fn main() -> Result<()> {
    let config = SessionConfig::new().set_str("datafusion.sql_parser.dialect", "postgres");
    let mut ctx = SessionContext::new_with_config(config);
    ctx.register_expr_planner(Arc::new(MyCustomPlanner))?;
    let results = ctx.sql("select 'foo'->'bar';").await?.collect().await?;

    let expected = [
         "+----------------------------+",
         "| Utf8(\"foo\") || Utf8(\"bar\") |",
         "+----------------------------+",
         "| foobar                     |",
         "+----------------------------+",
     ];
    assert_batches_eq!(&expected, &results);

    pretty::print_batches(&results)?;
    Ok(())
}
```
