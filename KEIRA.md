# Kerai

Kerai is the romanization of the Japanese word for someone in the service of another, describable as agent, staff, assistant, servant, etc.

It's used here to describe a collection of ever extensible tools presently consisting of several functionalities determined by invocation.

## A stack based language using either Infix (1 + 2), or Prefix (+ 1 2), or Postfix (1 2 +) for operator and operand expression.

Kerai is a slightly modal stack based language that reads from the input using whitespace as separators.
The default modality is quoted, where input is treated as an operand, or immediate, where it's treated as a operator.
Input is acted upon when a newline, or two dots in a row, is found in the stream, and a token is printable characters surrounded by whitespace.
In any input parenthesis can be used for grouping and calculations are done in the modal input of that line.

The default notation is Infix, so any input such as '1 + 2' (without the quote marks) will output the result of 3.
Operator precedence is standard algebra.

To use Prefix notation use a single dot as the first token in the input, so '. + 2 3' will output the result of 5.

To use Postfix notation use a single dot as the last token in the input, so '3 4 + .' will output the result of 7.

## A dotted directory in the user's home directory that stores startup programs that can serve as configurations.

## A program that orchestrates any of the functionality.

When invoked from the command line with no additional arguments, kerai runs any programs found in ~/.kerai and starts up as interactively. 

## A command line shell.

## A web application.

## A postgres database with a particular schema for intelligent storage of data in nodes.

## A postgres plugin allowing interface of other applications to the database.

## A distributed version control system, able to import and export and even replace a git repository.

