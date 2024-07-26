use proc_macro::TokenStream;

mod stack_error;

#[proc_macro_attribute]
pub fn stack_error(args: TokenStream, input: TokenStream) -> TokenStream {
    stack_error::stack_trace_style_impl(args.into(), input.into()).into()
}
